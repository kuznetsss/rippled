//! `host_abi!` — the single declaration point for the host/guest WASM ABI.
//!
//! Given a list of body-less function signatures, each annotated with the
//! required `#[gas = N]` (base gas cost) and `#[wasm = "name"]` (wasm import
//! name) attributes, this function-like proc-macro expands to:
//!
//! * a `HostFn` enum (one variant per function, PascalCase of the fn name)
//!   with a `const fn spec(self) -> HostFnSpec` and a `HostFn::ALL` slice;
//! * the `HostFunctions` trait, with `&self` prepended to each signature and
//!   the declared return type wrapped in `HostResult<_>` (bare `HostResult<()>`
//!   if none was declared). Doc comments on each entry are preserved on the
//!   trait method; the `#[gas]`/`#[wasm]` attributes are stripped;
//! * (wasm32 only) the **guest bindings**: a `#[link(wasm_import_module =
//!   "host")]` block declaring one raw import per function, and a `GuestHost`
//!   type implementing `HostFunctions` by marshaling Rust arguments into wasm
//!   scalars, calling the import, and decoding the return. These live behind
//!   `#[cfg(target_arch = "wasm32")]` so they compile only for the guest; the
//!   `stdlib` crate re-exports `GuestHost` from here. Generating them from the
//!   same declaration is what keeps the guest side from drifting: there is no
//!   longer a hand-written mirror of the ABI.
//!
//! The engine's import registration is *not* generated: `wasm_vm` walks
//! `HostFn::ALL` in an exhaustive `match` (so a new variant won't compile until
//! it's registered) and calls `func_wrap` per arm. That keeps all wasmi-facing
//! code in `wasm_vm` and this macro focused on the ABI declaration and the
//! (purely mechanical) guest side.
//!
//! ## The guest lowering the macro encodes
//!
//! Generating the guest bindings means this macro must know how each Rust type
//! lowers to wasm scalars — the mirror of `wasm_vm`'s `AbiArg`/`AbiRet`:
//!
//! | Rust (arg)     | wasm scalars passed                    |
//! |----------------|----------------------------------------|
//! | `i32` / `i64`  | the value                              |
//! | `bool`         | `x as i32`                             |
//! | `&[u8]` / `&str` | `(ptr, len)` into guest memory       |
//!
//! | Rust (return)  | import signature / decoding            |
//! |----------------|----------------------------------------|
//! | `()`           | returns `i32` status; `< 0` = error    |
//! | `u32`          | returns `i64`; `< 0` = error           |
//! | `Vec<u8>`      | caller passes its own `(out_ptr, out_len)`; the import writes into it and returns the `i32` byte count |
//! | `[u8; N]`      | caller passes its own `(out_ptr, out_len)`; the import writes into it and returns the `i32` byte count |
//!
//! A signature using a type outside this table is a compile error (on every
//! target, not just wasm32), so the guest side cannot silently fall behind.
//!
//! Note the shape shared by every value-producing return (`Vec<u8>` and
//! `[u8; N]` alike): rather than returning an owned buffer, its *trait* method
//! lowers to `fn(&self, .., out: &mut [u8]) -> HostResult<usize>` (bytes
//! written). The caller owns the buffer and the host writes straight into it,
//! so on the engine side the host's bytes go directly into guest linear memory
//! with no owned buffer to copy through — the whole point of the direct-write
//! path. A fixed-size `[u8; N]` is treated identically to a dynamic `Vec<u8>`
//! here; the declared size is documentation, and the engine still enforces the
//! buffer-fit / field-size / transfer policy from the returned length.
//!
//! The macro deliberately emits bare identifiers (`HostResult`, `HostError`,
//! `HostFnSpec`, `Vec`) rather than fully qualified paths: the call site is a
//! `#![no_std]` crate that has these in scope, and there is no way to name
//! `alloc::vec::Vec` generically without assuming the caller's exact import
//! style. This is a deliberate non-hygienic design, not an oversight. The
//! generated guest module re-establishes that scope with `use super::*;` plus
//! its own `use alloc::vec::Vec;`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Expr, ExprLit, FnArg, GenericArgument, Ident, Lit, Meta, Pat, PatType,
    PathArguments, ReturnType, TraitItemFn, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

/// Parses a `host_abi! { ... }` body as a sequence of body-less trait
/// function declarations (attrs + signature + trailing `;`), looping until
/// the input is exhausted. Using `syn::TraitItemFn` for each entry gets us
/// attribute parsing, doc comments, and the required semicolon for free.
struct HostAbiInput {
    fns: Vec<TraitItemFn>,
}

impl Parse for HostAbiInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut fns = Vec::new();
        while !input.is_empty() {
            fns.push(input.parse()?);
        }
        Ok(HostAbiInput { fns })
    }
}

/// Converts a `snake_case` identifier to `PascalCase` (e.g. `get_ledger_sqn`
/// -> `GetLedgerSqn`). Empty segments (from leading/trailing/doubled `_`) are
/// skipped rather than producing gaps.
fn to_pascal_case(ident: &str) -> String {
    ident
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Extracts the literal out of a `#[name = literal]` style attribute.
fn name_value_lit(attr: &Attribute) -> syn::Result<Lit> {
    match &attr.meta {
        Meta::NameValue(nv) => match &nv.value {
            Expr::Lit(ExprLit { lit, .. }) => Ok(lit.clone()),
            other => Err(syn::Error::new_spanned(
                other,
                "expected a literal value here",
            )),
        },
        other => Err(syn::Error::new_spanned(
            other,
            "expected a `name = value` attribute",
        )),
    }
}

/// Is `ty` the exact path `u8`?
fn is_u8(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident("u8"))
}

/// Is `ty` the exact single-segment path `name` (e.g. `i32`, `str`, `u32`)?
fn is_ident(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident(name))
}

/// Is `ty` `Vec<u8>` (matched by the last path segment, so `alloc::vec::Vec<u8>`
/// works too)?
fn is_vec_u8(ty: &Type) -> bool {
    let Type::Path(tp) = ty else { return false };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    if seg.ident != "Vec" {
        return false;
    }
    let PathArguments::AngleBracketed(ab) = &seg.arguments else {
        return false;
    };
    matches!(ab.args.first(), Some(GenericArgument::Type(inner)) if is_u8(inner))
}

/// One host-function argument, lowered to the wasm scalars the guest passes.
struct LoweredArg {
    /// Parameters for the raw `extern` import declaration, e.g.
    /// `data_ptr: i32, data_len: i32`.
    params: Vec<proc_macro2::TokenStream>,
    /// Expressions passed at the call site, e.g.
    /// `data.as_ptr() as usize as i32, data.len() as i32`.
    call: Vec<proc_macro2::TokenStream>,
}

/// Lower a single trait argument (`name: ty`) into its wasm scalar shape. This
/// is the guest-side twin of `wasm_vm`'s `AbiArg`.
fn lower_arg(name: &Ident, ty: &Type) -> syn::Result<LoweredArg> {
    // `&[u8]` / `&str` — a (ptr, len) pair into the guest's own linear memory.
    if let Type::Reference(r) = ty {
        let elem = &*r.elem;
        let is_bytes = matches!(elem, Type::Slice(s) if is_u8(&s.elem));
        if is_bytes || is_ident(elem, "str") {
            let ptr = format_ident!("{}_ptr", name);
            let len = format_ident!("{}_len", name);
            return Ok(LoweredArg {
                params: vec![quote! { #ptr: i32 }, quote! { #len: i32 }],
                call: vec![
                    quote! { #name.as_ptr() as usize as i32 },
                    quote! { #name.len() as i32 },
                ],
            });
        }
        return Err(syn::Error::new_spanned(
            ty,
            "host_abi! guest generation supports only `&[u8]` and `&str` reference arguments",
        ));
    }

    // Scalars.
    if is_ident(ty, "i32") {
        return Ok(LoweredArg {
            params: vec![quote! { #name: i32 }],
            call: vec![quote! { #name }],
        });
    }
    if is_ident(ty, "i64") {
        return Ok(LoweredArg {
            params: vec![quote! { #name: i64 }],
            call: vec![quote! { #name }],
        });
    }
    if is_ident(ty, "bool") {
        return Ok(LoweredArg {
            params: vec![quote! { #name: i32 }],
            call: vec![quote! { #name as i32 }],
        });
    }

    Err(syn::Error::new_spanned(
        ty,
        "host_abi! guest generation supports scalar args `i32`, `i64`, `bool` \
         and slice args `&[u8]`, `&str` only",
    ))
}

/// How a return type lowers on the guest side (the twin of `wasm_vm`'s `AbiRet`).
enum RetShape {
    /// `()` — import returns an `i32` status.
    Unit,
    /// `u32` — import returns an `i64` (value, or negative error code).
    ScalarU32,
    /// `Vec<u8>` or `[u8; N]` — a value-producing return. The host writes
    /// straight into a caller-provided buffer (a slice aliasing guest linear
    /// memory) and returns the value's true byte count as `i32` (`< 0` =
    /// error). Both the dynamic and the fixed-size forms lower to this one
    /// "fill-the-caller's-buffer" shape, so every value-producing host function
    /// writes directly into wasm memory with no owned intermediate to copy
    /// through.
    Bytes,
}

/// Classify a declared return type into its guest lowering, or `None` (with a
/// pushed error) if it is outside the supported set.
fn classify_return(output: &ReturnType) -> syn::Result<RetShape> {
    let ty = match output {
        ReturnType::Default => return Ok(RetShape::Unit),
        ReturnType::Type(_, ty) => &**ty,
    };
    if is_ident(ty, "u32") {
        Ok(RetShape::ScalarU32)
    } else if is_vec_u8(ty) {
        Ok(RetShape::Bytes)
    } else if let Type::Array(a) = ty {
        if is_u8(&a.elem) {
            Ok(RetShape::Bytes)
        } else {
            Err(syn::Error::new_spanned(
                ty,
                "host_abi! guest generation supports fixed arrays of `u8` only",
            ))
        }
    } else {
        Err(syn::Error::new_spanned(
            ty,
            "host_abi! guest generation supports returns `()`, `u32`, `Vec<u8>`, `[u8; N]` only",
        ))
    }
}

#[proc_macro]
pub fn host_abi(input: TokenStream) -> TokenStream {
    let HostAbiInput { fns } = parse_macro_input!(input as HostAbiInput);

    let mut variant_idents = Vec::new();
    let mut spec_arms = Vec::new();
    let mut trait_methods = Vec::new();
    let mut import_decls = Vec::new();
    let mut guest_methods = Vec::new();
    let mut errors: Vec<proc_macro2::TokenStream> = Vec::new();

    'entries: for item in fns {
        let TraitItemFn { attrs, mut sig, .. } = item;

        let mut gas_lit: Option<Lit> = None;
        let mut wasm_lit: Option<Lit> = None;
        let mut kept_attrs: Vec<Attribute> = Vec::new();

        for attr in attrs {
            if attr.path().is_ident("gas") {
                match name_value_lit(&attr) {
                    Ok(lit @ Lit::Int(_)) => gas_lit = Some(lit),
                    Ok(other) => errors.push(
                        syn::Error::new_spanned(other, "`#[gas]` must be an integer literal")
                            .to_compile_error(),
                    ),
                    Err(e) => errors.push(e.to_compile_error()),
                }
            } else if attr.path().is_ident("wasm") {
                match name_value_lit(&attr) {
                    Ok(lit @ Lit::Str(_)) => wasm_lit = Some(lit),
                    Ok(other) => errors.push(
                        syn::Error::new_spanned(other, "`#[wasm]` must be a string literal")
                            .to_compile_error(),
                    ),
                    Err(e) => errors.push(e.to_compile_error()),
                }
            } else {
                kept_attrs.push(attr);
            }
        }

        let fn_ident = sig.ident.clone();
        // Capture the untransformed signature before we prepend `&self` / wrap
        // the return type; the guest lowering is expressed in these originals.
        let orig_inputs = sig.inputs.clone();
        let orig_output = sig.output.clone();

        let gas_lit = match gas_lit {
            Some(lit) => lit,
            None => {
                errors.push(
                    syn::Error::new_spanned(
                        &fn_ident,
                        format!(
                            "host_abi! entry `{fn_ident}` is missing the required `#[gas = N]` attribute"
                        ),
                    )
                    .to_compile_error(),
                );
                continue;
            }
        };
        let wasm_lit = match wasm_lit {
            Some(lit) => lit,
            None => {
                errors.push(
                    syn::Error::new_spanned(
                        &fn_ident,
                        format!(
                            "host_abi! entry `{fn_ident}` is missing the required `#[wasm = \"name\"]` attribute"
                        ),
                    )
                    .to_compile_error(),
                );
                continue;
            }
        };

        let variant_ident = format_ident!("{}", to_pascal_case(&fn_ident.to_string()));

        spec_arms.push(quote! {
            HostFn::#variant_ident => HostFnSpec { name: #wasm_lit, base_gas: #gas_lit },
        });
        variant_idents.push(variant_ident);

        // Classify the return shape up front: it drives both the trait method
        // signature (here) and the guest lowering (below).
        let ret_shape = match classify_return(&orig_output) {
            Ok(shape) => shape,
            Err(e) => {
                errors.push(e.to_compile_error());
                continue 'entries;
            }
        };

        // Trait method: prepend `&self` and set the return type. A
        // value-producing return (`Vec<u8>` / `[u8; N]`) lowers to the
        // fill-the-caller's-buffer shape — an extra `out: &mut [u8]` parameter
        // and a `HostResult<usize>` (bytes written) return — so the engine can
        // have the host write straight into guest linear memory instead of
        // returning an owned buffer the engine must then copy in. Everything
        // else keeps its declared type, wrapped in `HostResult<_>`.
        let inner_ret: proc_macro2::TokenStream = match &sig.output {
            ReturnType::Default => quote! { () },
            ReturnType::Type(_, ty) => quote! { #ty },
        };
        if matches!(ret_shape, RetShape::Bytes) {
            sig.inputs.push(syn::parse_quote! { out: &mut [u8] });
            sig.output = syn::parse_quote! { -> HostResult<usize> };
        } else {
            sig.output = syn::parse_quote! { -> HostResult<#inner_ret> };
        }
        sig.inputs.insert(0, syn::parse_quote! { &self });
        let method_sig = sig.clone();

        trait_methods.push(quote! {
            #(#kept_attrs)*
            #sig;
        });

        // -- Guest binding for this entry -----------------------------------
        // Lower each argument, then the return shape; a type outside the
        // supported set is a hard error (recorded, aborts the whole macro).
        let mut arg_params = Vec::new();
        let mut call_args = Vec::new();
        for arg in &orig_inputs {
            let FnArg::Typed(PatType { pat, ty, .. }) = arg else {
                // Receivers can't appear: `host_abi!` entries are free fns.
                continue;
            };
            let Pat::Ident(pat_ident) = &**pat else {
                errors.push(
                    syn::Error::new_spanned(pat, "host_abi! arguments must be plain identifiers")
                        .to_compile_error(),
                );
                continue 'entries;
            };
            match lower_arg(&pat_ident.ident, ty) {
                Ok(LoweredArg { params, call }) => {
                    arg_params.extend(params);
                    call_args.extend(call);
                }
                Err(e) => {
                    errors.push(e.to_compile_error());
                    continue 'entries;
                }
            }
        }

        let import_ident = format_ident!("__hostimport_{}", fn_ident);
        let mut extern_params = arg_params;
        let (ret_scalar, body) = match ret_shape {
            RetShape::Unit => (
                quote! { i32 },
                quote! {
                    let __status = unsafe { #import_ident(#(#call_args),*) };
                    ret_unit(__status)
                },
            ),
            RetShape::ScalarU32 => (
                quote! { i64 },
                quote! {
                    let __status = unsafe { #import_ident(#(#call_args),*) };
                    ret_u32(__status)
                },
            ),
            RetShape::Bytes => {
                // The caller owns the output buffer (`out`, the trailing
                // `&mut [u8]` the trait lowering added); pass its base and
                // length to the import, which writes straight into it and
                // returns the value's true byte count.
                extern_params.push(quote! { out_ptr: i32 });
                extern_params.push(quote! { out_len: i32 });
                call_args.push(quote! { out.as_mut_ptr() as usize as i32 });
                call_args.push(quote! { out.len() as i32 });
                (
                    quote! { i32 },
                    quote! {
                        let __status = unsafe { #import_ident(#(#call_args),*) };
                        ret_bytes_len(__status)
                    },
                )
            }
        };

        import_decls.push(quote! {
            #[link_name = #wasm_lit]
            fn #import_ident(#(#extern_params),*) -> #ret_scalar;
        });
        guest_methods.push(quote! {
            #method_sig {
                #body
            }
        });
    }

    if !errors.is_empty() {
        return quote! { #(#errors)* }.into();
    }

    let expanded = quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum HostFn {
            #(#variant_idents),*
        }

        impl HostFn {
            pub const fn spec(self) -> HostFnSpec {
                match self {
                    #(#spec_arms)*
                }
            }

            pub const ALL: &'static [HostFn] = &[
                #(HostFn::#variant_idents),*
            ];
        }

        pub trait HostFunctions {
            #(#trait_methods)*
        }

        // Guest side: the same ABI, but each method *is* a wasm import call.
        // Compiled only for the wasm32 guest; `stdlib` re-exports `GuestHost`.
        #[cfg(target_arch = "wasm32")]
        #[doc(hidden)]
        #[allow(dead_code)]
        pub mod __guest_impl {
            use super::*;

            /// Decode a unit-returning import's status: `< 0` is an error code.
            #[inline]
            fn ret_unit(status: i32) -> HostResult<()> {
                if status < 0 {
                    Err(HostError::from_code(status))
                } else {
                    Ok(())
                }
            }

            /// Decode a `u32`-returning import's `i64` status.
            #[inline]
            fn ret_u32(status: i64) -> HostResult<u32> {
                if status < 0 {
                    Err(HostError::from_code(status as i32))
                } else {
                    Ok(status as u32)
                }
            }

            /// Decode a buffer-filling import (`Vec<u8>` / `[u8; N]` return):
            /// `status` is the count of bytes the host wrote into the
            /// caller-provided `out` buffer (`< 0` = error code).
            #[inline]
            fn ret_bytes_len(status: i32) -> HostResult<usize> {
                if status < 0 {
                    Err(HostError::from_code(status))
                } else {
                    Ok(status as usize)
                }
            }

            #[link(wasm_import_module = "host")]
            unsafe extern "C" {
                #(#import_decls)*
            }

            /// The production guest host: every method forwards to a wasm import.
            pub struct GuestHost;

            impl HostFunctions for GuestHost {
                #(#guest_methods)*
            }
        }

        #[cfg(target_arch = "wasm32")]
        pub use __guest_impl::GuestHost;
    };

    expanded.into()
}

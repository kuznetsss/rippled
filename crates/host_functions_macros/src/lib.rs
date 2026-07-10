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
//!   trait method; the `#[gas]`/`#[wasm]` attributes are stripped.
//!
//! The macro deliberately emits bare identifiers (`HostResult`, `HostError`,
//! `HostFnSpec`, `Vec`) rather than fully qualified paths: the call site is a
//! `#![no_std]` crate that has these in scope, and there is no way to name
//! `alloc::vec::Vec` generically without assuming the caller's exact import
//! style. This is a deliberate non-hygienic design, not an oversight.
//!
//! This first milestone only generates the enum + trait — no wasmi
//! registration, no guest-side import glue.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Attribute, Expr, ExprLit, Lit, Meta, ReturnType, TraitItemFn,
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
            other => Err(syn::Error::new_spanned(other, "expected a literal value here")),
        },
        other => Err(syn::Error::new_spanned(
            other,
            "expected a `name = value` attribute",
        )),
    }
}

#[proc_macro]
pub fn host_abi(input: TokenStream) -> TokenStream {
    let HostAbiInput { fns } = parse_macro_input!(input as HostAbiInput);

    let mut variant_idents = Vec::new();
    let mut spec_arms = Vec::new();
    let mut trait_methods = Vec::new();
    let mut errors: Vec<proc_macro2::TokenStream> = Vec::new();

    for item in fns {
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

        // Trait method: prepend `&self`, wrap the return type in `HostResult<_>`.
        let inner_ret: proc_macro2::TokenStream = match &sig.output {
            ReturnType::Default => quote! { () },
            ReturnType::Type(_, ty) => quote! { #ty },
        };
        sig.output = syn::parse_quote! { -> HostResult<#inner_ret> };
        sig.inputs.insert(0, syn::parse_quote! { &self });

        trait_methods.push(quote! {
            #(#kept_attrs)*
            #sig;
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
    };

    expanded.into()
}

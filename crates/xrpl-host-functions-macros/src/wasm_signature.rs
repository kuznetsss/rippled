//! The wasm signature a declaration describes: the one place a declared Rust type
//! becomes wasm parameters.
//!
//! Every other view of the ABI — what a host implements, what an engine registers,
//! what a guest imports — is that mapping read differently, so a type this module
//! does not know is rejected at the declaration rather than mishandled downstream.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{FnArg, Ident, Pat, PatType, Type, TypePath, TypeReference, TypeSlice};

/// The declared type of `trace`'s `data_type`, matched by name because the ABI
/// crate hand-writes it beside the declarations.
const TRACE_DATA_TYPE: &str = "TraceDataType";

/// Every type a parameter may be declared as, in the order the message reads best.
const ALLOWED: &str = "`&[u8]`, `&str`, `&mut [u8]`, `i32`, `i64`, `u32` or `TraceDataType`";

/// Which region a declared parameter is, for the parameters that cross as a
/// `(ptr, len)` pair rather than as a value.
///
/// Direction leads every name because it is the distinction that decides the rules:
/// an input is read and capped at the field limit, an output is written and clamped.
/// What follows it is what the bytes mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    /// `&[u8]`.
    InBytes,
    /// `&str`, so the engine's read is also the UTF-8 check.
    InStr,
    /// `u32` — four little-endian bytes in guest memory, **not** a wasm scalar, so
    /// it is two wasm parameters and `i32` is one. That is how the guest SDK passes
    /// a sequence number.
    InU32,
    /// `&mut [u8]`.
    OutBytes,
}

/// How a declared parameter crosses to the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// Spelled as itself, and passed as itself.
    I32,
    I64,
    /// An `i32` code the engine names before a host sees it.
    TraceType,
    Region(Region),
}

impl Encoding {
    /// The wasm value types this encoding occupies — a region is a `(ptr, len)` pair,
    /// everything else is one value — as tokens naming the ABI crate's
    /// `WasmValType`.
    ///
    /// Tokens rather than a value of that type: a proc-macro crate exports nothing
    /// but its macros, so the type the generated table is made of is one this crate
    /// cannot import and can only spell. It resolves at the expansion site, as
    /// `HostResult` in the declarations already does.
    pub(crate) fn wasm_types(self) -> Vec<TokenStream> {
        match self {
            Encoding::I64 => vec![quote!(WasmValType::I64)],
            Encoding::I32 | Encoding::TraceType => vec![quote!(WasmValType::I32)],
            Encoding::Region(_) => vec![quote!(WasmValType::I32); 2],
        }
    }
}

/// One declared parameter, and how it crosses.
pub(crate) struct Param {
    /// The name as declared. Load-bearing: it names the wasm parameters derived from
    /// it, so generated code reads like the declaration that produced it.
    #[allow(dead_code)]
    pub(crate) ident: Ident,
    pub(crate) encoding: Encoding,
}

/// What a host hands back, as the `T` of the declaration's `HostResult<T>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostReturn {
    /// `usize` — how long the value it wrote into the out buffer really is, whether
    /// or not that much fitted.
    BufferLength,
    /// `i32` — the answer itself, from a function that writes nothing.
    Value,
    /// `()` — and so the wasm function has no result at all.
    Nothing,
}

/// What the wasm function answers with — the wire side of [`HostReturn`], and all
/// this ABI ever puts there: one `i32`, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WasmResult {
    I32,
    Nothing,
}

impl From<HostReturn> for WasmResult {
    fn from(host_return: HostReturn) -> Self {
        match host_return {
            HostReturn::BufferLength | HostReturn::Value => WasmResult::I32,
            HostReturn::Nothing => WasmResult::Nothing,
        }
    }
}

impl WasmResult {
    /// The result as tokens, in the spelling [`Encoding::wasm_types`] uses. An
    /// `Option`, because that is the shape the generated table holds: a wasm function
    /// may have several results, and no declaration in this ABI has more than one.
    pub(crate) fn wasm_type(self) -> TokenStream {
        match self {
            WasmResult::I32 => quote!(Some(WasmValType::I32)),
            WasmResult::Nothing => quote!(None),
        }
    }
}

/// Reads one declared parameter as the guest sees it, or reports why it cannot
/// be read.
///
/// The receiver is not a wasm parameter and must be filtered out before this.
pub(crate) fn param(arg: &FnArg) -> syn::Result<Param> {
    let FnArg::Typed(PatType { pat, ty, .. }) = arg else {
        return Err(syn::Error::new_spanned(
            arg,
            "the receiver is not a wasm parameter",
        ));
    };

    // The two failures are independent, but an unnamed parameter has no useful type
    // error to pair with, so the name is checked first.
    let ident = binding(pat)?;
    Ok(Param {
        ident,
        encoding: encoding(ty)?,
    })
}

/// Reads the `T` of a declaration's `HostResult<T>`.
pub(crate) fn host_return(ty: &Type) -> syn::Result<HostReturn> {
    if let Type::Tuple(tuple) = ty
        && tuple.elems.is_empty()
    {
        return Ok(HostReturn::Nothing);
    }
    if let Some(name) = path_name(ty) {
        match name.as_str() {
            "usize" => return Ok(HostReturn::BufferLength),
            "i32" => return Ok(HostReturn::Value),
            _ => {}
        }
    }
    Err(syn::Error::new_spanned(
        ty,
        format!(
            "`{}` is not something a host function can return: `HostResult` carries \
             `usize` for a value written into an out buffer, `i32` for a value \
             returned directly, or `()` for a function whose wasm signature has no \
             result",
            spelled(ty)
        ),
    ))
}

/// The parameter's name, which the generated code needs.
fn binding(pat: &Pat) -> syn::Result<Ident> {
    // `mut x` is kept: it is part of the trait method emitted verbatim, and it does
    // not change the name anything is derived from.
    if let Pat::Ident(binding) = pat
        && binding.by_ref.is_none()
        && binding.subpat.is_none()
    {
        return Ok(binding.ident.clone());
    }
    Err(syn::Error::new_spanned(
        pat,
        "a host function's parameter must be a plain name: it names the wasm \
         parameters derived from it",
    ))
}

fn encoding(ty: &Type) -> syn::Result<Encoding> {
    match ty {
        Type::Path(TypePath {
            qself: None, path, ..
        }) => {
            // The last segment only, so a qualified `TraceDataType` is still recognised.
            let Some(last) = path.segments.last() else {
                return Err(unsupported(ty));
            };
            if !last.arguments.is_none() {
                return Err(unsupported(ty));
            }
            match last.ident.to_string().as_str() {
                "i32" => Ok(Encoding::I32),
                "i64" => Ok(Encoding::I64),
                "u32" => Ok(Encoding::Region(Region::InU32)),
                TRACE_DATA_TYPE => Ok(Encoding::TraceType),
                _ => Err(unsupported(ty)),
            }
        }
        Type::Reference(TypeReference {
            mutability, elem, ..
        }) => match &**elem {
            Type::Slice(TypeSlice { elem, .. }) if path_name(elem).as_deref() == Some("u8") => {
                Ok(Encoding::Region(if mutability.is_some() {
                    Region::OutBytes
                } else {
                    Region::InBytes
                }))
            }
            // `&mut str` falls through to the error: there is no writable string on
            // the wire.
            _ if mutability.is_none() && path_name(elem).as_deref() == Some("str") => {
                Ok(Encoding::Region(Region::InStr))
            }
            _ => Err(unsupported(ty)),
        },
        _ => Err(unsupported(ty)),
    }
}

fn unsupported(ty: &Type) -> syn::Error {
    syn::Error::new_spanned(
        ty,
        format!(
            "`{}` is not a wasm parameter type: a host function's parameter must be \
             {ALLOWED}",
            spelled(ty)
        ),
    )
}

/// The last segment of an unqualified path type, which is how the primitives and
/// `TraceDataType` are recognised.
fn path_name(ty: &Type) -> Option<String> {
    let Type::Path(TypePath {
        qself: None, path, ..
    }) = ty
    else {
        return None;
    };
    path.segments
        .last()
        .filter(|last| last.arguments.is_none())
        .map(|last| last.ident.to_string())
}

/// The type as written, for a diagnostic. `to_token_stream` spaces its tokens out
/// (`Vec < u8 >`), which reads badly in a message about the type the author wrote.
fn spelled(ty: &Type) -> String {
    ty.to_token_stream().to_string().replace(" ", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn read(arg: FnArg) -> syn::Result<Param> {
        param(&arg)
    }

    fn encoding_of(arg: FnArg) -> Encoding {
        read(arg).map(|p| p.encoding).unwrap()
    }

    fn rejection(arg: FnArg) -> String {
        let Err(error) = read(arg) else {
            panic!("expected the parameter to be rejected");
        };
        error.to_string()
    }

    #[test]
    fn reads_every_declared_type() {
        assert_eq!(encoding_of(parse_quote!(field: i32)), Encoding::I32);
        assert_eq!(encoding_of(parse_quote!(x: i64)), Encoding::I64);
        assert_eq!(
            encoding_of(parse_quote!(data_type: TraceDataType)),
            Encoding::TraceType
        );
        assert_eq!(
            encoding_of(parse_quote!(seq: u32)),
            Encoding::Region(Region::InU32)
        );
        assert_eq!(
            encoding_of(parse_quote!(account: &[u8])),
            Encoding::Region(Region::InBytes)
        );
        assert_eq!(
            encoding_of(parse_quote!(msg: &str)),
            Encoding::Region(Region::InStr)
        );
        assert_eq!(
            encoding_of(parse_quote!(out: &mut [u8])),
            Encoding::Region(Region::OutBytes)
        );
    }

    /// The declaration block may spell a vocabulary type through its crate, as it
    /// already may for `HostResult`.
    #[test]
    fn reads_a_qualified_trace_data_type() {
        assert_eq!(
            encoding_of(parse_quote!(data_type: xrpl_host_functions::TraceDataType)),
            Encoding::TraceType
        );
    }

    #[test]
    fn keeps_the_declared_name() {
        assert_eq!(read(parse_quote!(account: &[u8])).unwrap().ident, "account");
        assert_eq!(read(parse_quote!(mut seq: u32)).unwrap().ident, "seq");
    }

    /// The types one encoding occupies, as the emitted table spells them.
    fn wasm_types(encoding: Encoding) -> Vec<String> {
        encoding
            .wasm_types()
            .iter()
            .map(TokenStream::to_string)
            .collect()
    }

    #[test]
    fn a_region_counts_as_two_wasm_parameters_and_a_scalar_as_one() {
        for region in [
            Region::InBytes,
            Region::OutBytes,
            Region::InU32,
            Region::InStr,
        ] {
            assert_eq!(
                Encoding::Region(region).wasm_types().len(),
                2,
                "{region:?} is a (ptr, len) pair"
            );
        }
        for scalar in [Encoding::I32, Encoding::I64, Encoding::TraceType] {
            assert_eq!(scalar.wasm_types().len(), 1, "{scalar:?} is one value");
        }
    }

    /// The one place the ABI's two wasm value types are told apart: every region and
    /// every scalar but `i64` crosses as an `i32`.
    #[test]
    fn only_an_i64_parameter_crosses_as_an_i64() {
        assert_eq!(wasm_types(Encoding::I64), ["WasmValType :: I64"]);

        assert_eq!(wasm_types(Encoding::I32), ["WasmValType :: I32"]);
        assert_eq!(wasm_types(Encoding::TraceType), ["WasmValType :: I32"]);
        assert_eq!(
            wasm_types(Encoding::Region(Region::InBytes)),
            ["WasmValType :: I32", "WasmValType :: I32"]
        );
    }

    /// The check the ABI had none of: a type outside the table is a mistake at the
    /// declaration, not a surprise wherever it is next read.
    #[test]
    fn rejects_types_outside_the_table() {
        for (arg, spelled) in [
            (parse_quote!(data: Vec<u8>), "Vec<u8>"),
            (parse_quote!(n: u64), "u64"),
            (parse_quote!(flag: bool), "bool"),
            (parse_quote!(len: usize), "usize"),
            (parse_quote!(thing: &Foo), "&Foo"),
            (parse_quote!(hash: [u8; 32]), "[u8;32]"),
            (parse_quote!(byte: &mut u8), "&mutu8"),
            (parse_quote!(text: &mut str), "&mutstr"),
            (parse_quote!(words: &[&str]), "&[&str]"),
        ] {
            let message = rejection(arg);
            assert!(
                message.contains(&format!("`{spelled}` is not a wasm parameter type")),
                "{spelled}: {message}"
            );
            assert!(message.contains("`&[u8]`"), "{spelled}: {message}");
        }
    }

    /// The name is what the wasm parameters are derived from, so there must be one.
    #[test]
    fn rejects_parameters_without_a_name() {
        for arg in [
            parse_quote!(_: &[u8]),
            parse_quote!((a, b): &[u8]),
            parse_quote!(ref account: &[u8]),
        ] {
            assert!(
                rejection(arg).contains("must be a plain name"),
                "expected a naming diagnostic"
            );
        }
    }

    #[test]
    fn reads_every_host_return() {
        assert_eq!(
            host_return(&parse_quote!(usize)).unwrap(),
            HostReturn::BufferLength
        );
        assert_eq!(host_return(&parse_quote!(i32)).unwrap(), HostReturn::Value);
        assert_eq!(host_return(&parse_quote!(())).unwrap(), HostReturn::Nothing);
    }

    #[test]
    fn a_host_that_returns_nothing_has_no_wasm_result() {
        assert_eq!(WasmResult::from(HostReturn::BufferLength), WasmResult::I32);
        assert_eq!(WasmResult::from(HostReturn::Value), WasmResult::I32);
        assert_eq!(WasmResult::from(HostReturn::Nothing), WasmResult::Nothing);

        // And what that classification emits: an `Option`, because a wasm function
        // in this ABI answers with one value or with none.
        assert_eq!(
            WasmResult::I32.wasm_type().to_string(),
            "Some (WasmValType :: I32)"
        );
        assert_eq!(WasmResult::Nothing.wasm_type().to_string(), "None");
    }

    #[test]
    fn rejects_returns_the_wire_cannot_carry() {
        for (ty, spelled) in [
            (parse_quote!([u8; 4]), "[u8;4]"),
            (parse_quote!(bool), "bool"),
            (parse_quote!(u32), "u32"),
            (parse_quote!(i64), "i64"),
            (parse_quote!(Vec<u8>), "Vec<u8>"),
            (parse_quote!((i32, i32)), "(i32,i32)"),
        ] {
            let Err(error) = host_return(&ty) else {
                panic!("expected `{spelled}` to be rejected");
            };
            let message = error.to_string();
            assert!(
                message.contains(&format!(
                    "`{spelled}` is not something a host function can return"
                )),
                "{spelled}: {message}"
            );
            assert!(message.contains("`usize`"), "{spelled}: {message}");
        }
    }
}

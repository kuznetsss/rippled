use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Block, FnArg, GenericArgument, Ident, LitInt, LitStr, PathArguments, ReceiverKind,
    ReturnType, Safety, Signature, TraitItemFn, Type, TypePath, parse::Parse,
};

use crate::errors;
use crate::wasm_signature::{self, Encoding, HostReturn, Param, Region, Results};

/// `#[gas = N]`: the base gas charged before the call runs.
const GAS: &str = "gas";
/// `#[wasm_name = "..."]`: the name the guest imports the function under.
const WASM_NAME: &str = "wasm_name";
/// `///` desugars to `#[doc = "..."]` before macro expansion.
const DOC: &str = "doc";
/// The alias every declaration returns its success type through.
const HOST_RESULT: &str = "HostResult";

/// One entry of a `host_functions!` block: its ABI metadata and its signature.
pub(crate) struct ParsedHostFunction {
    pub(crate) gas: u64,
    /// Kept as the literal the user wrote, so diagnostics and the generated
    /// string both carry that span.
    pub(crate) wasm_name: LitStr,
    /// Doc comments, in source order, to re-emit on the generated items.
    pub(crate) docs: Vec<Attribute>,
    /// The enum variant this declaration becomes, spanned at the function name.
    pub(crate) variant: Ident,
    /// The declaration as written, which the trait method is emitted from.
    pub(crate) signature: Signature,
    /// The same parameters as the guest sees them. Declaration order is wasm
    /// parameter order, so this is `signature.inputs` without the receiver.
    // Deriving these is what validates a declaration against the wire; the
    // generated items are emitted from `signature` and the ABI attributes.
    #[allow(dead_code)]
    pub(crate) wasm_params: Vec<Param>,
    #[allow(dead_code)]
    pub(crate) wasm_result: Results,
}

impl ParsedHostFunction {
    /// `#[doc …] fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;`
    pub(crate) fn trait_method(&self) -> TokenStream {
        let docs = &self.docs;
        // The declaration is already a trait method: emitted verbatim, so what
        // the block reads like is what the trait is.
        let signature = &self.signature;

        quote! {
            #(#docs)*
            #signature;
        }
    }

    /// `#[doc …] GetLedgerSqn`
    pub(crate) fn variant_declaration(&self) -> TokenStream {
        let docs = &self.docs;
        let variant = &self.variant;
        quote! {
            #(#docs)*
            #variant
        }
    }

    /// `Self::GetLedgerSqn => HostFnSpec { name: "ldgr_index", gas: 60u64 }`
    pub(crate) fn spec_arm(&self) -> TokenStream {
        let Self {
            gas,
            wasm_name,
            variant,
            ..
        } = self;
        quote! {
            Self::#variant => HostFnSpec { name: #wasm_name, gas: #gas }
        }
    }

    /// Every mistake in one declaration is collected before any is reported, so a
    /// block is not fixed one diagnostic per build.
    ///
    /// The four steps below are also the order the mistakes are reported in, which
    /// `reports_mistakes_in_a_fixed_order` pins: what the declaration is tagged
    /// with, whether it is a declaration at all, what it means on the wire, and
    /// what it becomes. A reader working top-down through one function's
    /// diagnostics meets them in that order whatever else is wrong.
    pub(crate) fn parse(function: TraitItemFn) -> syn::Result<Self> {
        let TraitItemFn {
            attrs,
            sig,
            default,
            ..
        } = function;
        let mut errors = Vec::new();

        let attributes = Attributes::parse(attrs, &sig.ident, &mut errors);
        check_declaration(&sig, default.as_ref(), &mut errors);
        let wasm = wasm_signature_of(&sig, &mut errors);

        // A name whose PascalCase form is not a legal variant is reported here
        // rather than emitted, which would either panic or fail downstream.
        let variant = match variant_ident(&sig.ident) {
            Ok(variant) => Some(variant),
            Err(error) => {
                errors.push(error);
                None
            }
        };

        if let Some(error) = errors::combine(errors) {
            return Err(error);
        }

        let (Some(gas), Some(wasm_name), Some(variant), Some((wasm_params, wasm_result))) =
            (attributes.gas, attributes.wasm_name, variant, wasm)
        else {
            unreachable!("every absent field is reported above");
        };

        Ok(Self {
            gas,
            wasm_name,
            docs: attributes.docs,
            variant,
            signature: sig,
            wasm_params,
            wasm_result,
        })
    }
}

/// The ABI attributes one declaration carries, and the doc comments to re-emit.
struct Attributes {
    gas: Option<u64>,
    wasm_name: Option<LitStr>,
    docs: Vec<Attribute>,
}

impl Attributes {
    /// `ident` is where an absent attribute is reported, there being no attribute
    /// to point at.
    fn parse(attrs: Vec<Attribute>, ident: &Ident, errors: &mut Vec<syn::Error>) -> Self {
        let mut parsed = Attributes {
            gas: None,
            wasm_name: None,
            docs: Vec::new(),
        };

        // Tracked separately from the values so a malformed attribute is not also
        // reported as a missing one.
        let mut saw_gas = false;
        let mut saw_wasm_name = false;

        for attr in attrs {
            if attr.path().is_ident(GAS) {
                saw_gas = true;
                if let Err(error) =
                    int_value(&attr).and_then(|v| set_once(&mut parsed.gas, v, &attr))
                {
                    errors.push(error);
                }
            } else if attr.path().is_ident(WASM_NAME) {
                saw_wasm_name = true;
                if let Err(error) = value::<LitStr>(&attr, "a string literal")
                    .and_then(|v| set_once(&mut parsed.wasm_name, v, &attr))
                {
                    errors.push(error);
                }
            } else if attr.path().is_ident(DOC) {
                parsed.docs.push(attr);
            } else {
                errors.push(syn::Error::new_spanned(
                    &attr,
                    format!("unexpected attribute `{}`", path_name(&attr)),
                ));
            }
        }

        if !saw_gas {
            errors.push(syn::Error::new_spanned(
                ident,
                format!("missing `#[{GAS} = ...]` attribute"),
            ));
        }
        if !saw_wasm_name {
            errors.push(syn::Error::new_spanned(
                ident,
                format!("missing `#[{WASM_NAME} = \"...\"]` attribute"),
            ));
        }
        if let Some(name) = &parsed.wasm_name {
            errors.extend(check_wasm_name(name).err());
        }

        parsed
    }
}

/// What makes a declaration one: a plain `fn` over `&self`, with no body and no
/// generics, because it maps to exactly one wasm import signature.
fn check_declaration(signature: &Signature, body: Option<&Block>, errors: &mut Vec<syn::Error>) {
    if let Some(body) = body {
        errors.push(syn::Error::new_spanned(
            body,
            "a host function is implemented by the host, so it must not have a body",
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &signature.ident,
            "a host function must not be generic: it maps to one wasm import signature",
        ));
    }
    errors.extend(check_receiver(signature).err());
    reject_modifiers(signature, errors);
}

/// The declaration as the guest sees it: the parameters without the receiver, and
/// the wasm result the return type gives.
///
/// The two are read together because the rule between them — a length is reported
/// only into an out buffer — needs both. `None` always comes with a reported error.
fn wasm_signature_of(
    signature: &Signature,
    errors: &mut Vec<syn::Error>,
) -> Option<(Vec<Param>, Results)> {
    let declared: Vec<&FnArg> = signature
        .inputs
        .iter()
        .filter(|arg| !matches!(arg, FnArg::Receiver(_)))
        .collect();

    let mut params = Vec::with_capacity(declared.len());
    for arg in &declared {
        match wasm_signature::param(arg) {
            Ok(param) => params.push(param),
            Err(error) => errors.push(error),
        }
    }

    let host_return = match returned_type(signature).and_then(wasm_signature::host_return) {
        Ok(host_return) => host_return,
        Err(error) => {
            errors.push(error);
            return None;
        }
    };

    // A parameter that could not be read is absent from `params`, and the rule below
    // would report it a second time as a missing out buffer.
    if params.len() != declared.len() {
        return None;
    }
    errors.extend(check_result_against_out_buffers(signature, host_return, &params).err());

    Some((params, host_return.into()))
}

/// A `usize` return is the length of a value the host wrote, so it says nothing
/// unless there is a buffer it was written into. The two are declared together or
/// neither is.
///
/// This is what makes the wasm signature derivable from the declaration alone:
/// which helper marshals a function, and whether it reports a length, is read off
/// the parameters and the return type agreeing.
fn check_result_against_out_buffers(
    signature: &Signature,
    host_return: HostReturn,
    params: &[Param],
) -> syn::Result<()> {
    let out_buffers = params
        .iter()
        .filter(|param| matches!(param.encoding, Encoding::Region(Region::OutBytes)))
        .count();

    match (host_return, out_buffers) {
        (HostReturn::BufferLength, 0) => Err(syn::Error::new_spanned(
            &signature.output,
            "a host function returning `HostResult<usize>` reports the length of a \
             value it wrote, so it must take an out buffer: `out: &mut [u8]`",
        )),
        (HostReturn::Value | HostReturn::Nothing, 1..) => Err(syn::Error::new_spanned(
            &signature.output,
            "a host function taking an out buffer must return `HostResult<usize>`: \
             the length is how a guest whose buffer was too small learns the size to \
             ask for",
        )),
        _ => Ok(()),
    }
}

/// Every declaration carries a receiver, and it is always `&self`.
///
/// `&self` is the only receiver that can work: the VM reaches the host through a
/// shared `&dyn HostFunctions` stored in the wasmi `Store`, and a host that needs
/// to mutate does so behind interior mutability. The receiver is not part of the
/// wasm ABI — the guest passes no `self` — so it is uniform across the block.
fn check_receiver(signature: &Signature) -> syn::Result<()> {
    let Some(receiver) = signature.receiver() else {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            format!(
                "a host function must declare its receiver: `fn {}(&self, ...)`",
                signature.ident
            ),
        ));
    };

    // `&self` and nothing else: not `&mut self`, not `self`/`mut self`, not a
    // typed `self: Box<Self>`, and not a spelled-out lifetime.
    if !matches!(receiver.kind, ReceiverKind::Reference(_, None, None)) {
        return Err(syn::Error::new_spanned(
            receiver,
            "a host function's receiver must be exactly `&self`: the VM calls the host \
             through a shared `&dyn HostFunctions`",
        ));
    }
    Ok(())
}

/// The `T` of every declaration's `HostResult<T>`, including the `()` of the ones
/// that yield nothing.
///
/// One shape for every function is what lets a single dispatch adapter lower them
/// all: lift the arguments out of guest memory, call the host, then turn `Ok(T)`
/// into the wire's non-negative `i32` and `Err(e)` into a negative code or a trap.
/// A function returning a bare `T` would need its own arm.
fn returned_type(signature: &Signature) -> syn::Result<&Type> {
    const SHAPE: &str = "a host function must return `HostResult<T>` — \
                         `HostResult<()>` if it yields nothing";

    let ReturnType::Type(_, returned) = &signature.output else {
        return Err(syn::Error::new_spanned(&signature.ident, SHAPE));
    };

    let Type::Path(TypePath {
        qself: None, path, ..
    }) = &**returned
    else {
        return Err(syn::Error::new_spanned(returned, SHAPE));
    };
    // The last segment only, so `HostResult<T>` may be written qualified.
    let Some(last) = path.segments.last() else {
        return Err(syn::Error::new_spanned(returned, SHAPE));
    };
    if last.ident != HOST_RESULT {
        return Err(syn::Error::new_spanned(returned, SHAPE));
    }

    // `HostResult` without its success type is `HostResult` the alias, which names
    // no type; rustc's own message for that is unhelpfully far from the cause.
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return Err(syn::Error::new_spanned(
            returned,
            format!("`{HOST_RESULT}` needs its success type: `{HOST_RESULT}<T>`"),
        ));
    };
    if arguments.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            arguments,
            format!("`{HOST_RESULT}` takes exactly one type: `{HOST_RESULT}<T>`"),
        ));
    }
    let Some(GenericArgument::Type(success)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            arguments,
            format!("`{HOST_RESULT}` takes a type, not a lifetime or a constant"),
        ));
    };
    Ok(success)
}

/// `const`, `async`, `unsafe`/`safe` and `extern "…"` have no meaning in the
/// wasm ABI, and would otherwise pass silently into the generated trait.
fn reject_modifiers(signature: &Signature, errors: &mut Vec<syn::Error>) {
    const PLAIN: &str =
        "a host function must be a plain `fn`: this modifier is not part of the wasm ABI";

    if let Some(constness) = &signature.constness {
        errors.push(syn::Error::new_spanned(constness, PLAIN));
    }
    if let Some(asyncness) = &signature.asyncness {
        errors.push(syn::Error::new_spanned(asyncness, PLAIN));
    }
    match &signature.safety {
        Safety::Default => {}
        Safety::Safe(token) => errors.push(syn::Error::new_spanned(token, PLAIN)),
        Safety::Unsafe(token) => errors.push(syn::Error::new_spanned(token, PLAIN)),
    }
    if let Some(abi) = &signature.abi {
        errors.push(syn::Error::new_spanned(abi, PLAIN));
    }
}

/// The wasm import name reaches the engine's import table verbatim, so it is
/// held to what an import name can sanely be rather than to any string.
fn check_wasm_name(name: &LitStr) -> syn::Result<()> {
    let value = name.value();
    if value.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "the wasm name must not be empty",
        ));
    }
    if let Some(character) = value
        .chars()
        .find(|c| !c.is_ascii_alphanumeric() && *c != '_')
    {
        return Err(syn::Error::new_spanned(
            name,
            format!(
                "a wasm name may only contain `A-Za-z0-9_`, but this one contains {character:?}"
            ),
        ));
    }
    Ok(())
}

/// The enum variant a declaration becomes: `get_ledger_sqn` -> `GetLedgerSqn`.
///
/// The result carries `ident`'s span, so anything the compiler says about the
/// variant points at the declaration that produced it.
fn variant_ident(ident: &Ident) -> syn::Result<Ident> {
    // `to_string` spells raw identifiers `r#type`; the `r#` is not part of the name.
    let name = ident.to_string();
    let name = name.strip_prefix("r#").unwrap_or(&name);

    let mut pascal = String::with_capacity(name.len());
    let mut capitalize = true;
    for character in name.chars() {
        if character == '_' {
            capitalize = true;
        } else if capitalize {
            pascal.extend(character.to_uppercase());
            capitalize = false;
        } else {
            pascal.push(character);
        }
    }

    // A name of nothing but underscores leaves `pascal` empty; the original is
    // already a legal identifier, so keep it.
    if pascal.is_empty() {
        return Ok(ident.clone());
    }

    // `Ident::new` panics on a leading digit (`_2fa` -> `2fa`) and silently
    // accepts keyword spellings (`self_` -> `Self`), which then fails to parse
    // where the variant is emitted. Parsing rejects both, without panicking.
    if let Err(error) = syn::parse_str::<Ident>(&pascal) {
        return Err(syn::Error::new_spanned(
            ident,
            format!(
                "this name becomes the enum variant `{pascal}`, which is not a valid \
                 variant name ({error}); rename the host function"
            ),
        ));
    }
    Ok(format_ident!("{pascal}", span = ident.span()))
}

/// Records `value`, or reports that the attribute appeared more than once.
fn set_once<T>(slot: &mut Option<T>, value: T, attr: &Attribute) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        return Err(syn::Error::new_spanned(
            attr,
            format!("duplicate `{}` attribute", path_name(attr)),
        ));
    }
    Ok(())
}

/// The value of `#[name = <value>]`, parsed as `T`.
///
/// `expected` completes "`gas` expects …": syn's own message for the wrong kind
/// of literal names neither the attribute nor what it wanted.
fn value<T: Parse>(attr: &Attribute, expected: &str) -> syn::Result<T> {
    let expr = &attr.meta.require_name_value()?.value;
    syn::parse2(expr.to_token_stream()).map_err(|_| {
        syn::Error::new_spanned(expr, format!("`{}` expects {expected}", path_name(attr)))
    })
}

fn int_value(attr: &Attribute) -> syn::Result<u64> {
    let int: LitInt = value(attr, "an integer literal")?;
    // `LitInt` keeps the sign in its digits, so `base10_parse::<u64>` would
    // report a negative value as "invalid digit found in string".
    if int.base10_digits().starts_with('-') {
        return Err(syn::Error::new_spanned(
            int,
            format!("`{}` must not be negative", path_name(attr)),
        ));
    }
    int.base10_parse()
}

/// The attribute's path as written, for diagnostics: `gas`, or `foo::bar`.
fn path_name(attr: &Attribute) -> String {
    attr.path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{Expr, ExprLit, Lit, parse_quote};

    /// The message of every diagnostic recorded by one failed `parse`.
    ///
    /// `expect_err` is unavailable here: it needs `T: Debug`, and syn only
    /// implements `Debug` for its AST types under the `extra-traits` feature.
    fn messages(function: TraitItemFn) -> Vec<String> {
        let Err(error) = ParsedHostFunction::parse(function) else {
            panic!("expected parsing to fail");
        };
        error.into_iter().map(|error| error.to_string()).collect()
    }

    fn doc_text(attr: &Attribute) -> String {
        match &attr.meta.require_name_value().unwrap().value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) => text.value(),
            _ => panic!("doc attribute is not a string literal"),
        }
    }

    #[test]
    fn reads_gas_and_wasm_name() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();

        assert_eq!(parsed.gas, 60);
        assert_eq!(parsed.wasm_name.value(), "ldgr_index");
        assert_eq!(parsed.signature.ident.to_string(), "get_ledger_sqn");
        assert_eq!(parsed.variant.to_string(), "GetLedgerSqn");
        assert!(parsed.docs.is_empty());
    }

    #[test]
    fn derives_variant_names_from_function_names() {
        for (function, variant) in [
            ("get_ledger_sqn", "GetLedgerSqn"),
            ("sha512_half", "Sha512Half"),
            ("trace", "Trace"),
            ("get_current_ledger_obj_field", "GetCurrentLedgerObjField"),
            ("r#type", "Type"),
            ("trace2", "Trace2"),
            // Pathological, but must not panic: no letters to capitalize.
            ("__", "__"),
        ] {
            let ident = format_ident!("{function}");
            assert_eq!(
                variant_ident(&ident).map(|v| v.to_string()).ok(),
                Some(variant.to_owned()),
                "{function}"
            );
        }
    }

    /// `_2fa` would PascalCase to `2fa`; building that `Ident` panics, and a
    /// panic in a proc macro is reported with no useful span at all.
    #[test]
    fn rejects_a_name_that_becomes_a_leading_digit() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "two_factor"]
            fn _2fa(&self) -> HostResult<()>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("becomes the enum variant `2fa`"),
            "{messages:?}"
        );
    }

    /// `self_` PascalCases to `Self`, which `Ident::new` accepts and rustc then
    /// rejects where the variant is emitted. `r#Self` is not a legal escape.
    #[test]
    fn rejects_a_name_that_becomes_a_keyword() {
        for function in ["self_", "_self"] {
            let ident = format_ident!("{function}");
            let Err(error) = variant_ident(&ident) else {
                panic!("expected `{function}` to be rejected");
            };
            assert!(
                error.to_string().contains("variant `Self`"),
                "{}",
                error.to_string()
            );
        }
    }

    #[test]
    fn rejects_negative_gas() {
        let messages = messages(parse_quote! {
            #[gas = -5]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(messages[0], "`gas` must not be negative");
    }

    #[test]
    fn rejects_unusable_wasm_names() {
        let empty = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = ""]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(empty.len(), 1, "{empty:?}");
        assert_eq!(empty[0], "the wasm name must not be empty");

        let spaced = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(spaced.len(), 1, "{spaced:?}");
        assert!(spaced[0].contains("may only contain"), "{spaced:?}");
    }

    #[test]
    fn rejects_signature_modifiers() {
        for declaration in [
            quote! { unsafe fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>; },
            quote! { async fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>; },
            quote! { const fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>; },
            quote! { extern "C" fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>; },
        ] {
            let function: TraitItemFn = syn::parse2(quote! {
                #[gas = 60]
                #[wasm_name = "ldgr_index"]
                #declaration
            })
            .unwrap();

            let messages = messages(function);
            assert_eq!(messages.len(), 1, "{messages:?}");
            assert!(messages[0].contains("must be a plain `fn`"), "{messages:?}");
        }
    }

    #[test]
    fn trait_method_keeps_the_declared_receiver_and_ends_in_a_semicolon() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            /// Hashes `data`.
            #[gas = 2000]
            #[wasm_name = "sha512_half"]
            fn sha512_half(&self, data: &[u8], out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();

        // `///` reaches the macro as `#[doc = r"..."]`: rustc's lexer spells doc
        // comments as raw string literals.
        let method = parsed.trait_method().to_string();
        assert!(
            method.starts_with("# [doc = r\" Hashes `data`.\"]"),
            "{method}"
        );
        assert!(
            method
                .contains("fn sha512_half (& self , data : & [u8] , out : & mut [u8]) -> HostResult < usize > ;"),
            "{method}"
        );
    }

    #[test]
    fn spec_arm_carries_the_name_and_the_gas() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();

        assert_eq!(
            parsed.spec_arm().to_string(),
            "Self :: GetLedgerSqn => HostFnSpec { name : \"ldgr_index\" , gas : 60u64 }"
        );
    }

    #[test]
    fn keeps_doc_comments_in_source_order() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            /// First line.
            ///
            /// Third line.
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();

        let docs: Vec<_> = parsed.docs.iter().map(doc_text).collect();
        assert_eq!(docs, vec![" First line.", "", " Third line."]);
    }

    #[test]
    fn preserves_parameters_and_return_type() {
        let traced = ParsedHostFunction::parse(parse_quote! {
            #[gas = 500]
            #[wasm_name = "trace"]
            fn trace(&self, msg: &str, data_type: TraceDataType, data: &[u8]) -> HostResult<()>;
        })
        .unwrap();
        // The receiver is `inputs[0]`; the three wasm parameters follow it.
        assert_eq!(traced.signature.inputs.len(), 4);
        assert_eq!(
            traced.signature.output.to_token_stream().to_string(),
            "-> HostResult < () >"
        );

        let hashed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 2000]
            #[wasm_name = "sha512_half"]
            fn sha512_half(&self, data: &[u8], out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();
        assert_eq!(
            hashed.signature.output.to_token_stream().to_string(),
            "-> HostResult < usize >"
        );
    }

    #[test]
    fn reports_both_missing_attributes_at_once() {
        let messages = messages(parse_quote! {
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("missing `#[gas"), "{messages:?}");
        assert!(messages[1].contains("missing `#[wasm_name"), "{messages:?}");
    }

    #[test]
    fn names_the_unexpected_attribute() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wsam_name = "typo"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        // The typo'd attribute, plus the `wasm_name` it failed to be.
        assert_eq!(messages.len(), 2);
        assert!(
            messages.iter().any(|m| m.contains("`wsam_name`")),
            "{messages:?}"
        );
    }

    #[test]
    fn rejects_wrong_literal_types() {
        let gas = messages(parse_quote! {
            #[gas = "60"]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(gas.len(), 1, "{gas:?}");
        assert!(
            gas[0].contains("`gas` expects an integer literal"),
            "{gas:?}"
        );

        let name = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = 7]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(name.len(), 1, "{name:?}");
        assert!(
            name[0].contains("`wasm_name` expects a string literal"),
            "{name:?}"
        );
    }

    #[test]
    fn rejects_gas_that_does_not_fit_in_u64() {
        let messages = messages(parse_quote! {
            #[gas = 99999999999999999999999]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("number too large"), "{messages:?}");
    }

    #[test]
    fn rejects_attribute_shapes_other_than_name_value() {
        let bare = messages(parse_quote! {
            #[gas]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(bare.len(), 1, "{bare:?}");
        assert!(bare[0].contains("gas = ..."), "{bare:?}");

        let list = messages(parse_quote! {
            #[gas(60)]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(list.len(), 1, "{list:?}");
    }

    #[test]
    fn rejects_duplicate_attributes() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[gas = 70]
            #[wasm_name = "ldgr_index"]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("duplicate `gas`"), "{messages:?}");
        assert!(
            messages[1].contains("duplicate `wasm_name`"),
            "{messages:?}"
        );
    }

    /// A malformed attribute must not also be reported as an absent one.
    #[test]
    fn does_not_report_a_malformed_attribute_as_missing() {
        let messages = messages(parse_quote! {
            #[gas = "60"]
            #[wasm_name = 7]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            !messages.iter().any(|m| m.contains("missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn rejects_a_body() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> { Ok(0) }
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("must not have a body"), "{messages:?}");
    }

    #[test]
    fn rejects_generics() {
        let parameter = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn<T>(&self, out: &mut [u8]) -> HostResult<usize>;
        });
        assert_eq!(parameter.len(), 1, "{parameter:?}");
        assert!(
            parameter[0].contains("must not be generic"),
            "{parameter:?}"
        );

        let clause = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> where Self: Sized;
        });
        assert_eq!(clause.len(), 1, "{clause:?}");
    }

    #[test]
    fn requires_a_receiver() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("must declare its receiver: `fn get_ledger_sqn(&self, ...)`"),
            "{messages:?}"
        );
    }

    /// Anything but `&self` would need a host the VM cannot hand out: it holds
    /// one shared `&dyn HostFunctions` for the whole run.
    #[test]
    fn rejects_receivers_other_than_shared_self() {
        for receiver in [
            quote! { &mut self },
            quote! { self },
            quote! { mut self },
            quote! { self: Box<Self> },
            quote! { &'a self },
        ] {
            let function: TraitItemFn = syn::parse2(quote! {
                #[gas = 60]
                #[wasm_name = "ldgr_index"]
                fn get_ledger_sqn(#receiver, out: &mut [u8]) -> HostResult<usize>;
            })
            .unwrap_or_else(|_| panic!("`{receiver}` should parse"));

            let messages = messages(function);
            assert_eq!(messages.len(), 1, "`{receiver}`: {messages:?}");
            assert!(
                messages[0].contains("must be exactly `&self`"),
                "`{receiver}`: {messages:?}"
            );
        }
    }

    /// A bare `T` return would need its own lowering arm, so the uniform shape is
    /// required rather than inferred.
    #[test]
    fn rejects_returns_that_are_not_host_result() {
        for output in [
            quote! {},
            quote! { -> () },
            quote! { -> [u8; 4] },
            quote! { -> i32 },
            quote! { -> Result<[u8; 4], HostError> },
            quote! { -> impl Iterator<Item = u8> },
        ] {
            let function: TraitItemFn = syn::parse2(quote! {
                #[gas = 60]
                #[wasm_name = "ldgr_index"]
                fn get_ledger_sqn(&self) #output;
            })
            .unwrap_or_else(|_| panic!("`{output}` should parse"));

            let messages = messages(function);
            assert_eq!(messages.len(), 1, "`{output}`: {messages:?}");
            assert!(
                messages[0].contains("must return `HostResult<T>`"),
                "`{output}`: {messages:?}"
            );
        }
    }

    /// `HostResult` may be written qualified, since the trait method keeps whatever
    /// path resolves where the block is written.
    #[test]
    fn accepts_a_qualified_host_result() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: &mut [u8]) -> xrpl_host_functions::HostResult<usize>;
        })
        .unwrap();

        assert!(
            parsed
                .trait_method()
                .to_string()
                .contains("xrpl_host_functions :: HostResult < usize >"),
            "{}",
            parsed.trait_method()
        );
    }

    /// `HostResult` with no success type names no type at all; rustc's own error
    /// for that lands on the generated trait, far from the declaration.
    #[test]
    fn rejects_host_result_without_a_success_type() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self) -> HostResult;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("needs its success type"),
            "{messages:?}"
        );
    }

    /// The declared parameters and the wasm ones are not the same list: three
    /// parameters here are six on the wire, and that is the count nothing else in
    /// the tree used to know.
    #[test]
    fn records_the_wasm_signature() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 350]
            #[wasm_name = "check_id"]
            fn check_keylet(&self, account: &[u8], seq: u32, out: &mut [u8]) -> HostResult<usize>;
        })
        .unwrap();

        assert_eq!(
            parsed
                .wasm_params
                .iter()
                .map(|param| param.encoding)
                .collect::<Vec<_>>(),
            vec![
                Encoding::Region(Region::InBytes),
                Encoding::Region(Region::InU32),
                Encoding::Region(Region::OutBytes),
            ],
        );
        assert_eq!(total_wasm_params(&parsed), 6);
        assert_eq!(parsed.wasm_result, Results::I32);
    }

    /// `trace`'s shape: a `TraceDataType` is one wasm parameter rather than two,
    /// and returning `()` is the only thing that empties the result list.
    #[test]
    fn records_a_declaration_with_no_wasm_result() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 30]
            #[wasm_name = "trace"]
            fn trace(&self, msg: &str, data_type: TraceDataType, data: &[u8]) -> HostResult<()>;
        })
        .unwrap();

        assert_eq!(total_wasm_params(&parsed), 5);
        assert_eq!(parsed.wasm_result, Results::Empty);
    }

    /// A `usize` is the length of a value the host wrote, so a declaration that
    /// returns one without offering a buffer to write into says nothing.
    #[test]
    fn rejects_a_length_return_with_no_out_buffer() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("must take an out buffer"),
            "{messages:?}"
        );
    }

    #[test]
    fn rejects_an_out_buffer_without_a_length_return() {
        for output in [quote! { HostResult<i32> }, quote! { HostResult<()> }] {
            let function: TraitItemFn = syn::parse2(quote! {
                #[gas = 60]
                #[wasm_name = "ldgr_index"]
                fn get_ledger_sqn(&self, out: &mut [u8]) -> #output;
            })
            .unwrap();

            let messages = messages(function);
            assert_eq!(messages.len(), 1, "`{output}`: {messages:?}");
            assert!(
                messages[0].contains("must return `HostResult<usize>`"),
                "`{output}`: {messages:?}"
            );
        }
    }

    /// The rule is that there is a buffer at all, not how many: `float_to_mant_exp`
    /// declares two.
    #[test]
    fn accepts_more_than_one_out_buffer() {
        let parsed = ParsedHostFunction::parse(parse_quote! {
            #[gas = 400]
            #[wasm_name = "float_to_mant_exp"]
            fn float_to_mant_exp(
                &self,
                float: &[u8],
                mantissa: &mut [u8],
                exponent: &mut [u8],
            ) -> HostResult<usize>;
        })
        .unwrap();

        assert_eq!(total_wasm_params(&parsed), 6);
    }

    #[test]
    fn reports_every_unusable_parameter() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, a: Vec<u8>, b: bool, out: &mut [u8]) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("`Vec<u8>`"), "{messages:?}");
        assert!(messages[1].contains("`bool`"), "{messages:?}");
    }

    /// One mistake, one diagnostic: the parameter that could not be read is the one
    /// the out-buffer rule would otherwise report a second time as absent.
    #[test]
    fn does_not_report_an_unusable_parameter_as_a_missing_out_buffer() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr_index"]
            fn get_ledger_sqn(&self, out: Vec<u8>) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("not a wasm parameter type"),
            "{messages:?}"
        );
    }

    /// A declaration wrong in four ways at once reports all four, in the order
    /// `parse` runs its steps. Nothing downstream depends on the order, but a
    /// reader does, and it is otherwise decided by accident.
    #[test]
    fn reports_mistakes_in_a_fixed_order() {
        let messages = messages(parse_quote! {
            #[gas = 60]
            #[wasm_name = "ldgr index"]
            async fn get_ledger_sqn<T>(&self, data: Vec<u8>) -> HostResult<usize>;
        });

        assert_eq!(messages.len(), 4, "{messages:?}");
        // The attributes, then the declaration's shape, then the wire, then the
        // name it becomes.
        assert!(messages[0].contains("may only contain"), "{messages:?}");
        assert!(messages[1].contains("must not be generic"), "{messages:?}");
        assert!(messages[2].contains("must be a plain `fn`"), "{messages:?}");
        assert!(
            messages[3].contains("not a wasm parameter type"),
            "{messages:?}"
        );
    }

    /// Neither half of the rule fires when a function returns a value and writes
    /// into nothing, which is what the 13 `HostResult<i32>` declarations are.
    #[test]
    fn accepts_a_value_return_with_no_out_buffer() {
        for output in [quote! { HostResult<i32> }, quote! { HostResult<()> }] {
            let function: TraitItemFn = syn::parse2(quote! {
                #[gas = 60]
                #[wasm_name = "ldgr_index"]
                fn get_ledger_sqn(&self, locator: &[u8]) -> #output;
            })
            .unwrap();

            ParsedHostFunction::parse(function)
                .unwrap_or_else(|_| panic!("`{output}` with no out buffer should be accepted"));
        }
    }

    fn total_wasm_params(parsed: &ParsedHostFunction) -> usize {
        parsed
            .wasm_params
            .iter()
            .map(|param| param.encoding.wasm_param_count())
            .sum()
    }
}

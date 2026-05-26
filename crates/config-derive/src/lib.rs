//! Procedural macros for the `config` crate.
//!
//! `#[derive(ConfigEntries)]` walks a struct's named fields and generates a
//! getter for each one in a flavor that matches the field's type:
//!
//! | Field type                  | Generated getter signature                              |
//! | --------------------------- | ------------------------------------------------------- |
//! | `Option<u8/u16/u32/u64/i32/bool>` | `fn name(&self) -> Box<<option_ns>::OptionalT>`   |
//! | `Option<String>` / `Option<PathBuf>` | `fn name(&self) -> Box<<option_ns>::OptionalString>` |
//! | `Vec<String>`               | `fn name(&self) -> &[String]`                           |
//! | `Option<Vec<String>>`       | `fn name(&self) -> &[String]` (empty when `None`)       |
//! | `Option<T>` (struct)        | `fn name(&self) -> Result<&T, String>` + `fn has_name(&self) -> bool` |
//! | `T` (plain struct)          | `fn name(&self) -> &T`                                  |
//!
//! Fields tagged `#[config_entry(skip)]` are ignored — write the FFI shape by
//! hand. Unsupported field types must be skipped or the derive errors with a
//! message naming the field.
//!
//! ## Struct-level attribute
//!
//! `#[config_entries(option_ns = "::path::to::ffi")]` overrides the module
//! path the derive uses for `OptionalT` shared structs. Default is
//! `crate::ffi`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Field, Fields, GenericArgument, Path,
    PathArguments, Type,
};

#[proc_macro_derive(ConfigEntries, attributes(config_entry, config_entries))]
pub fn derive_config_entries(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let option_ns = match parse_option_ns(&input.attrs) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error().into(),
    };

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return syn::Error::new_spanned(
                    name,
                    "ConfigEntries only supports structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(name, "ConfigEntries only supports structs")
                .to_compile_error()
                .into();
        }
    };

    let mut getters = Vec::new();
    for field in fields {
        if has_skip_attr(&field.attrs) {
            continue;
        }
        match build_getter(field, &option_ns) {
            Ok(tokens) => getters.push(tokens),
            Err(err) => return err.to_compile_error().into(),
        }
    }

    let expanded = quote! {
        impl #name {
            #(#getters)*
        }
    };
    expanded.into()
}

/// Parse `#[config_entries(option_ns = "...")]` off the struct's attributes.
/// Defaults to `crate::ffi`.
fn parse_option_ns(attrs: &[Attribute]) -> syn::Result<Path> {
    let default: Path = syn::parse_str("crate::ffi").expect("default path parses");
    let mut result = default;

    for attr in attrs {
        if !attr.path().is_ident("config_entries") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("option_ns") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                let parsed: Path = syn::parse_str(&lit.value()).map_err(|e| {
                    syn::Error::new(lit.span(), format!("invalid module path: {e}"))
                })?;
                result = parsed;
                Ok(())
            } else {
                Err(meta.error("unknown `config_entries` argument; expected `option_ns`"))
            }
        })?;
    }
    Ok(result)
}

fn has_skip_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("config_entry") {
            return false;
        }
        let mut is_skip = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                is_skip = true;
            }
            Ok(())
        });
        is_skip
    })
}

fn build_getter(field: &Field, option_ns: &Path) -> syn::Result<TokenStream2> {
    let name = field
        .ident
        .as_ref()
        .ok_or_else(|| syn::Error::new_spanned(field, "unnamed field unsupported"))?;
    let ty = &field.ty;

    if let Some(inner) = extract_generic(ty, "Option") {
        return option_getter(name, inner, option_ns);
    }
    if let Some(inner) = extract_generic(ty, "Vec") {
        return vec_getter(name, inner);
    }
    // Plain `T` (a non-Option non-Vec type). Treat as a nested struct ref.
    Ok(quote! {
        pub fn #name(&self) -> &#ty {
            &self.#name
        }
    })
}

fn option_getter(name: &syn::Ident, inner: &Type, option_ns: &Path) -> syn::Result<TokenStream2> {
    let type_name = type_last_ident_string(inner);
    match type_name.as_deref() {
        Some("bool") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalBool> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("u8") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalU8> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("u16") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalU16> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("u32") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalU32> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("u64") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalU64> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("i32") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalI32> {
                ::std::boxed::Box::new(self.#name.into())
            }
        }),
        Some("String") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalString> {
                ::std::boxed::Box::new(self.#name.clone().into())
            }
        }),
        Some("PathBuf") => Ok(quote! {
            pub fn #name(&self) -> ::std::boxed::Box<#option_ns::OptionalString> {
                ::std::boxed::Box::new(
                    self.#name
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .into(),
                )
            }
        }),
        _ => {
            // Option<Vec<String>> → &[String] with empty fallback.
            if let Some(vec_inner) = extract_generic(inner, "Vec") {
                if type_last_ident_string(vec_inner).as_deref() == Some("String") {
                    return Ok(quote! {
                        pub fn #name(&self) -> &[::std::string::String] {
                            match &self.#name {
                                Some(v) => v.as_slice(),
                                None => &[],
                            }
                        }
                    });
                }
                return Err(syn::Error::new_spanned(
                    inner,
                    format!(
                        "ConfigEntries: `Option<Vec<_>>` is only supported for `Vec<String>` (field `{name}`). Use #[config_entry(skip)]."
                    ),
                ));
            }

            // Otherwise treat as Option<NestedStruct>: a throwable ref getter
            // paired with a non-throwing `has_X()` presence check.
            let has_ident = format_ident!("has_{}", name);
            Ok(quote! {
                pub fn #has_ident(&self) -> bool {
                    self.#name.is_some()
                }
                pub fn #name(&self) -> ::std::result::Result<&#inner, ::std::string::String> {
                    self.#name.as_ref().ok_or_else(|| format!(
                        "config: `{}` is not set", stringify!(#name)
                    ))
                }
            })
        }
    }
}

fn vec_getter(name: &syn::Ident, inner: &Type) -> syn::Result<TokenStream2> {
    if type_last_ident_string(inner).as_deref() == Some("String") {
        return Ok(quote! {
            pub fn #name(&self) -> &[::std::string::String] {
                &self.#name
            }
        });
    }
    Err(syn::Error::new_spanned(
        inner,
        format!(
            "ConfigEntries: only `Vec<String>` is supported (field `{name}`). Use #[config_entry(skip)] for other Vec types."
        ),
    ))
}

fn extract_generic<'a>(ty: &'a Type, ident: &str) -> Option<&'a Type> {
    let Type::Path(p) = ty else {
        return None;
    };
    let last = p.path.segments.last()?;
    if last.ident != ident {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| {
        if let GenericArgument::Type(t) = arg {
            Some(t)
        } else {
            None
        }
    })
}

fn type_last_ident_string(ty: &Type) -> Option<String> {
    if let Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

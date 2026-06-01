use crate::config_builder::ConfigBuilder;
use crate::error::{FinalizeOutcome, ParseOutcome};
use crate::ffi::bridge::ConfigFormat as FfiConfigFormat;
use crate::ConfigFormat;

/// Map the FFI `ConfigFormat` enum to the Rust-side `ConfigFormat`.
fn to_config_format(fmt: FfiConfigFormat) -> ConfigFormat {
    match fmt {
        FfiConfigFormat::Toml => ConfigFormat::Toml,
        FfiConfigFormat::Ini => ConfigFormat::Ini,
        // cxx enums are non-exhaustive in generated code; this branch is
        // unreachable for any value the C++ side can legally construct.
        _ => ConfigFormat::Toml,
    }
}

pub(crate) fn parse_from_str(content: &str, format: FfiConfigFormat) -> Box<ParseOutcome> {
    ParseOutcome::from_builder_result(crate::parse_from_str(content, to_config_format(format)))
}

pub(crate) fn parse_from_file(path: &str) -> Box<ParseOutcome> {
    ParseOutcome::from_builder_result(crate::parse_from_file(path))
}

/// Finalize a `ConfigBuilder` (apply CLI flags → normalize → validate).
///
/// Takes the builder by value via `Box`.  cxx cannot call consuming `self`
/// methods, so this is a free function.
pub(crate) fn finalize(b: Box<ConfigBuilder>) -> Box<FinalizeOutcome> {
    FinalizeOutcome::from_result(b.finalize())
}

use crate::error::ParseOutcome;
use crate::ffi::bridge::ConfigFormat as FfiConfigFormat;
use crate::LoadOptions;
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

pub(crate) fn parse_from_str(
    content: &str,
    format: FfiConfigFormat,
    opts: &LoadOptions,
) -> Box<ParseOutcome> {
    let result = crate::parse_from_str(content, to_config_format(format), opts.clone());
    ParseOutcome::from_ini_result(result)
}

pub(crate) fn parse_from_file(path: &str, opts: &LoadOptions) -> Box<ParseOutcome> {
    ParseOutcome::from_ini_result(crate::parse_from_file(path, opts.clone()))
}

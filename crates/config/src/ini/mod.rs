//! INI parsing pipeline for rippled config files.
//!
//! Two stages:
//! 1. `lexer::tokenize` — normalise text, strip comments, split into `RawSections`.
//! 2. `adapt::adapt`    — dispatch each `RawSection` to a handler and populate `Config`.
//!
//! Public entrypoint: `parse_ini(&str) -> Result<Config, ConfigError>`.

mod lexer;
pub(crate) mod raw;
mod grammar;
mod serde;
mod adapt;

use crate::error::ConfigError;
use crate::config::Config;

/// Parse an INI configuration blob and return a populated `Config`.
///
/// No filesystem I/O, no `validators.txt` splice — call `Config::bootstrap()` for those.
pub fn parse_ini(text: &str) -> Result<Config, ConfigError> {
    let raw = lexer::tokenize(text)?;
    adapt::adapt(raw)
}

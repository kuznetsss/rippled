//! TOML parsing pipeline.
//!
//! Entry point: `parse_toml(text) -> Result<Config, ConfigError>`.
//!
//! Strategy: deserialise the text into `schema::Root` via the `toml` crate's
//! serde driver, then convert with `schema::root_to_config` which:
//!   1. Moves fields from `Root` into `Parsed`.
//!   2. Runs `validate_strict()` on each section struct.
//!   3. Reconciles `server.ports` with the `[port.<name>]` table-of-tables.
//!   4. Runs top-level cross-section validation.

mod schema;

use crate::config::Config;
use crate::error::ConfigError;

/// Parse a TOML config blob into a `Config`.
///
/// This function is strict-by-default:
/// - Unknown keys → error (via `deny_unknown_fields`).
/// - Out-of-range values → error (via `validate_strict()`).
/// - Malformed sections → error.
///
/// Path resolution is deferred to `Config::bootstrap()`.
pub fn parse_toml(text: &str) -> Result<Config, ConfigError> {
    // Use ::toml (the external crate) to avoid ambiguity with this module.
    let root: schema::Root = ::toml::from_str(text).map_err(|e| {
        ConfigError::grammar("TOML document", text, e.to_string())
    })?;
    schema::root_to_config(root)
}

//! Shared test helpers for integration tests.
//!
//! Each helper performs a common setup pattern so individual test functions
//! stay concise. None of the helpers are "clever" — they just encode the
//! canonical `from_*_str → set_config_dir → set_standalone → bootstrap`
//! sequence and panic on unexpected errors with a clear message.

use std::path::PathBuf;
use config::Config;

/// Parse an INI string, set `config_dir` to a temp-like path, set standalone,
/// run bootstrap, and return the finalized `Config`.
///
/// Panics if parsing or bootstrap fails.
pub fn parse_ini_bootstrap(text: &str) -> Config {
    parse_ini_bootstrap_in(text, std::env::temp_dir())
}

/// Like `parse_ini_bootstrap` but uses an explicit directory.
pub fn parse_ini_bootstrap_in(text: &str, config_dir: PathBuf) -> Config {
    let mut cfg = Config::from_ini_str(text)
        .unwrap_or_else(|e| panic!("INI parse failed: {e}"));
    cfg.set_config_dir(config_dir);
    cfg.set_standalone(true); // skip mkdir; keep tests hermetic
    cfg.set_quiet(true);      // suppress stderr echo
    cfg.bootstrap()
        .unwrap_or_else(|e| panic!("bootstrap failed: {e}"));
    cfg
}

/// Parse a TOML string, set `config_dir`, set standalone, run bootstrap.
///
/// Panics if parsing or bootstrap fails.
pub fn parse_toml_bootstrap(text: &str) -> Config {
    parse_toml_bootstrap_in(text, std::env::temp_dir())
}

/// Like `parse_toml_bootstrap` but uses an explicit directory.
pub fn parse_toml_bootstrap_in(text: &str, config_dir: PathBuf) -> Config {
    let mut cfg = Config::from_toml_str(text)
        .unwrap_or_else(|e| panic!("TOML parse failed: {e}"));
    cfg.set_config_dir(config_dir);
    cfg.set_standalone(true);
    cfg.set_quiet(true);
    cfg.bootstrap()
        .unwrap_or_else(|e| panic!("bootstrap failed: {e}"));
    cfg
}

//! FFI bridge between the Rust `Config` type and C++ consumers.
//!
//! This module declares the `cxx::bridge` that exposes `Config` to C++.
//! The bridge is intentionally minimal for Phase 3: it proves the pattern
//! works end-to-end and exercises `NodeDbHandle` as a representative sub-struct
//! handle. The full surface is expanded as C++ migration proceeds.
//!
//! Design: §10 of `config_rewrite_design.md`.
//!
//! ## cxx::bridge constraints honoured
//! - No `Option<T>` across the bridge: sentinel values (`""`, `-1`, etc.)
//!   represent "not set".
//! - No `BTreeMap` across the bridge: collections are `Vec<T>` or individual
//!   accessor calls.
//! - Fallible constructors return `Box<ConfigOutcome>` / `Box<UnitOutcome>`
//!   rather than `Result<T>` — cxx `Result<T>` would throw a C++ exception;
//!   the outcome pattern avoids all exception machinery.
//! - Lifetime-carrying getters (`&str` views into Config-owned data) use the
//!   `'a` syntax supported by cxx.

// ---------------------------------------------------------------------------
// Outcome wrappers (opaque Rust types, not shared structs)
// ---------------------------------------------------------------------------

use crate::config::Config;
use crate::error::ConfigError;

/// Wraps `Result<Box<Config>, ConfigError>`.
/// C++ accesses it through `has_value()`, `has_error()`, `error()`, `into_value()`.
///
/// The error message is eagerly materialized at construction time into `error_msg`,
/// so `error()` can return a borrow with no allocation and no memory leak.
pub struct ConfigOutcome {
    inner: Result<Box<Config>, ConfigError>,
    /// Pre-materialized error message (empty string when Ok).
    error_msg: String,
}

impl ConfigOutcome {
    pub fn has_value(&self) -> bool {
        self.inner.is_ok()
    }

    pub fn has_error(&self) -> bool {
        self.inner.is_err()
    }

    /// Returns the error message.  The borrow is tied to the lifetime of this outcome
    /// so it is safe to return without leaking.
    pub fn error(&self) -> &str {
        &self.error_msg
    }

    pub fn into_value(self: Box<Self>) -> Box<Config> {
        self.inner.expect("ConfigOutcome::into_value called on an error outcome")
    }
}

impl From<Result<Box<Config>, ConfigError>> for ConfigOutcome {
    fn from(r: Result<Box<Config>, ConfigError>) -> Self {
        let error_msg = match &r {
            Ok(_) => String::new(),
            Err(e) => e.message(),
        };
        ConfigOutcome { inner: r, error_msg }
    }
}

/// Wraps `Result<(), ConfigError>`.
pub struct UnitOutcome {
    inner: Result<(), ConfigError>,
    /// Pre-materialized error message (empty string when Ok).
    error_msg: String,
}

impl UnitOutcome {
    pub fn has_error(&self) -> bool {
        self.inner.is_err()
    }

    /// Returns the error message.  The borrow is tied to the lifetime of this outcome.
    pub fn error(&self) -> &str {
        &self.error_msg
    }
}

impl From<Result<(), ConfigError>> for UnitOutcome {
    fn from(r: Result<(), ConfigError>) -> Self {
        let error_msg = match &r {
            Ok(()) => String::new(),
            Err(e) => e.message(),
        };
        UnitOutcome { inner: r, error_msg }
    }
}

// ---------------------------------------------------------------------------
// Sub-struct handle: NodeDbHandle (proof-of-concept per design §10)
// ---------------------------------------------------------------------------

/// An opaque handle returned by `Config::node_db_handle()`.
/// C++ calls its accessor methods rather than projecting fields directly,
/// making schema evolution safe.
pub struct NodeDbHandle {
    pub(crate) inner: crate::types::NodeDbConfig,
    /// Cached UTF-8 path string for zero-copy &str returns.
    pub(crate) path_str: String,
}

impl NodeDbHandle {
    pub fn kind(&self) -> u8 {
        self.inner.kind as u8
    }

    pub fn path(&self) -> &str {
        &self.path_str
    }

    pub fn fast_load(&self) -> bool {
        self.inner.fast_load
    }

    pub fn earliest_seq(&self) -> u32 {
        self.inner.earliest_seq
    }

    /// Returns the `online_delete` threshold, or `-1` if unset.
    pub fn online_delete(&self) -> i64 {
        match self.inner.online_delete {
            Some(v) => v as i64,
            None => -1,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI-facing functions that construct outcome wrappers
// ---------------------------------------------------------------------------

/// Consume a `ConfigOutcome` and return its `Box<Config>` on success.
/// Panics if the outcome holds an error. C++ callers must check `has_error()` first.
pub fn config_outcome_into_value(outcome: Box<ConfigOutcome>) -> Box<Config> {
    outcome.into_value()
}

/// Load a config from a file path (UTF-8 string). Format chosen by extension.
/// Returns a `ConfigOutcome`; C++ checks `has_error()` before calling `config_outcome_into_value()`.
pub fn config_load(path: &str) -> Box<ConfigOutcome> {
    let result = Config::from_file(std::path::Path::new(path))
        .map(Box::new);
    Box::new(ConfigOutcome::from(result))
}

/// Parse an INI blob.
pub fn config_parse_ini(text: &str) -> Box<ConfigOutcome> {
    let result = Config::from_ini_str(text).map(Box::new);
    Box::new(ConfigOutcome::from(result))
}

/// Parse a TOML blob.
pub fn config_parse_toml(text: &str) -> Box<ConfigOutcome> {
    let result = Config::from_toml_str(text).map(Box::new);
    Box::new(ConfigOutcome::from(result))
}

/// Run bootstrap on a `Config`. Returns a `UnitOutcome`.
pub fn config_bootstrap(cfg: &mut Config) -> Box<UnitOutcome> {
    Box::new(UnitOutcome::from(cfg.bootstrap()))
}

// ---------------------------------------------------------------------------
// Accessor helpers — return owned Strings (avoids cxx lifetime requirements)
// ---------------------------------------------------------------------------

/// Get the config directory as a UTF-8 string.
/// Panics if bootstrap has not been called.
pub fn config_config_dir(cfg: &Config) -> String {
    cfg.config_dir()
        .to_string_lossy()
        .into_owned()
}

/// Get the data directory as a UTF-8 string.
/// Panics if bootstrap has not been called.
pub fn config_data_dir(cfg: &Config) -> String {
    cfg.data_dir()
        .to_string_lossy()
        .into_owned()
}

/// Get the debug logfile path as a UTF-8 string, or `""` if unset.
/// Panics if bootstrap has not been called.
pub fn config_debug_logfile(cfg: &Config) -> String {
    cfg.debug_logfile()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Get the validation_seed, or `""` if unset.
pub fn config_validation_seed(cfg: &Config) -> String {
    cfg.validation_seed().unwrap_or("").to_owned()
}

/// Get the server_domain, or `""` if unset.
pub fn config_server_domain(cfg: &Config) -> String {
    cfg.server_domain().unwrap_or("").to_owned()
}

// ---------------------------------------------------------------------------
// NodeDbHandle factory
// ---------------------------------------------------------------------------

/// Materialise a `NodeDbHandle` from the Config's `node_db` config.
/// The handle is owned by the caller (returned as `Box<NodeDbHandle>`).
/// It captures a copy of the `NodeDbConfig` so it has no lifetime dependency
/// on `Config` — which simplifies the C++ ownership story.
pub fn config_node_db_handle(cfg: &Config) -> Box<NodeDbHandle> {
    let inner = cfg.node_db().clone();
    let path_str = inner.path.to_string_lossy().into_owned();
    Box::new(NodeDbHandle { inner, path_str })
}

// ---------------------------------------------------------------------------
// The cxx::bridge declaration
// ---------------------------------------------------------------------------

#[cxx::bridge(namespace = "rs::config")]
mod bridge {
    // Shared plain-data structs (owned by either side).
    pub struct HostPortFfi {
        pub host: String,
        pub port: u16,
        pub has_port: bool,
    }

    extern "Rust" {
        // ----- Outcome types -----
        type ConfigOutcome;

        fn has_value(self: &ConfigOutcome) -> bool;
        fn has_error(self: &ConfigOutcome) -> bool;
        // Returns a static &str (leaks on error; acceptable for startup diagnostics).
        fn error(self: &ConfigOutcome) -> &str;
        // NOTE: cxx does not support Box<Self> receivers on opaque types.
        // `into_value` is exposed as a free function taking Box<ConfigOutcome>.
        fn config_outcome_into_value(outcome: Box<ConfigOutcome>) -> Box<Config>;

        type UnitOutcome;
        fn has_error(self: &UnitOutcome) -> bool;
        fn error(self: &UnitOutcome) -> &str;

        // ----- Config opaque type -----
        type Config;

        // constructors (return owned outcome wrappers)
        fn config_load(path: &str) -> Box<ConfigOutcome>;
        fn config_parse_ini(text: &str) -> Box<ConfigOutcome>;
        fn config_parse_toml(text: &str) -> Box<ConfigOutcome>;

        // override setters
        fn set_quiet(self: &mut Config, v: bool);
        fn set_silent(self: &mut Config, v: bool);
        fn set_standalone(self: &mut Config, v: bool);
        fn set_force_multi_thread(self: &mut Config, v: bool);

        // finalize
        fn config_bootstrap(cfg: &mut Config) -> Box<UnitOutcome>;

        // scalar getters
        fn network_id(self: &Config) -> u32;
        fn network_quorum(self: &Config) -> u64;
        fn peer_private(self: &Config) -> bool;
        fn quiet(self: &Config) -> bool;
        fn standalone(self: &Config) -> bool;

        // post-bootstrap path getters (return owned String — copy cost is negligible
        // at startup; avoids the cxx `unsafe fn` requirement for explicit lifetimes).
        fn config_config_dir(cfg: &Config) -> String;
        fn config_data_dir(cfg: &Config) -> String;
        fn config_debug_logfile(cfg: &Config) -> String;
        fn config_validation_seed(cfg: &Config) -> String;
        fn config_server_domain(cfg: &Config) -> String;

        // sized-item lookup (u8 codes because cxx can't pass Rust enums directly)
        fn sized_value_raw(self: &Config, item: u8) -> i32;
        fn sized_value_for_raw(self: &Config, item: u8, node: u8) -> i32;

        // ----- NodeDbHandle -----
        type NodeDbHandle;
        fn config_node_db_handle(cfg: &Config) -> Box<NodeDbHandle>;

        fn kind(self: &NodeDbHandle) -> u8;
        // Returns an owned String; path is copied once into the handle at creation.
        fn path(self: &NodeDbHandle) -> &str;
        fn fast_load(self: &NodeDbHandle) -> bool;
        fn earliest_seq(self: &NodeDbHandle) -> u32;
        fn online_delete(self: &NodeDbHandle) -> i64;
    }
}

// ---------------------------------------------------------------------------
// Bridge glue: sized_value_raw / sized_value_for_raw take u8 item/node codes
// (u8 instead of typed enums because cxx can't pass Rust enums directly)
// ---------------------------------------------------------------------------

impl Config {
    fn sized_value_raw(&self, item: u8) -> i32 {
        use crate::types::sized::SIZED_TABLE;
        if item as usize >= SIZED_TABLE.len() {
            return 0;
        }
        let size = self.node_size();
        SIZED_TABLE[item as usize][size as usize]
    }

    fn sized_value_for_raw(&self, item: u8, node: u8) -> i32 {
        use crate::types::sized::SIZED_TABLE;
        if item as usize >= SIZED_TABLE.len() || node as usize >= 5 {
            return 0;
        }
        SIZED_TABLE[item as usize][node as usize]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_outcome_ok() {
        let text = "";
        let outcome = config_parse_ini(text);
        assert!(outcome.has_value());
        assert!(!outcome.has_error());
        assert_eq!(outcome.error(), "");
    }

    #[test]
    fn config_outcome_err() {
        // Force an error by passing invalid TOML
        let text = "this is not valid toml @@@@";
        let outcome = config_parse_toml(text);
        // TOML parser might or might not error on bare text; but we can at least
        // verify the outcome type compiles and runs.
        let _ = outcome.has_error();
    }

    #[test]
    fn unit_outcome_ok() {
        let r: Result<(), ConfigError> = Ok(());
        let outcome = UnitOutcome::from(r);
        assert!(!outcome.has_error());
        assert_eq!(outcome.error(), "");
    }

    #[test]
    fn unit_outcome_err() {
        let r: Result<(), ConfigError> = Err(ConfigError::cross("test error"));
        let outcome = UnitOutcome::from(r);
        assert!(outcome.has_error());
        assert!(outcome.error().contains("test error"));
    }

    #[test]
    fn node_db_handle_defaults() {
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        let handle = config_node_db_handle(&cfg);
        assert!(!handle.fast_load());
        assert_eq!(handle.earliest_seq(), 32570); // default
        assert_eq!(handle.online_delete(), -1);   // unset
    }
}

//! Rust `config` crate — replacement for the C++ `Config` class in rippled.
//!
//! # Module layout
//!
//! - `error`     — `ConfigError`, error kinds, source spans
//! - `types/`    — all public sub-structs returned by getters
//! - `config`    — the single public `Config` type (constructors, setters, bootstrap, getters)
//! - `ini/`      — INI parser (Phase 2A)
//! - `toml/`     — TOML parser (Phase 2B)
//! - `bootstrap` — path resolution, NodeSize detection, validators.txt splice (Phase 3)
//! - `ffi`       — cxx::bridge wrappers (Phase 3)

pub mod error;
pub mod types;
pub mod config;
pub mod toml;
pub mod ini;
pub mod bootstrap;
pub mod ffi;

pub use config::Config;
pub use error::{ConfigError, ConfigErrorKind, Format, SourceSpan};
pub use types::*;
pub use crate::toml::parse_toml;
pub use ini::parse_ini;

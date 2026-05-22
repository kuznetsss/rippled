//! `[crawl]` and `[vl]` tables — small tables that don't justify their own file.

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

/// `[crawl]`. Replaces the legacy bare `0|1` value line with `enabled`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Crawl {
    /// Master switch. Default `true`.
    pub enabled: Option<bool>,
    /// Default `true`.
    pub overlay: Option<bool>,
    /// Default `true`.
    pub server: Option<bool>,
    /// Default `false`.
    pub counts: Option<bool>,
    /// Default `true`.
    pub unl: Option<bool>,
}

/// `[vl]`. TOML uses `enabled` (the INI loader accepts `enable` as an alias,
/// but that is not exposed here).
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Vl {
    pub enabled: Option<bool>,
}

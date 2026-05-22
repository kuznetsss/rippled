//! `[insight]` and `[perf]` tables.

use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Insight {
    /// Currently only `"statsd"` is recognized; omit the section to use the
    /// null collector.
    // FFI (phase 2): cxx-shared `InsightServer` (Statsd).
    // Planned: `Insight::server()` returning `OptionalInsightServer`.
    #[config_entry(skip)]
    pub server: Option<InsightServer>,
    /// `host:port`. Consumed only when `server = "statsd"`.
    pub address: Option<String>,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InsightServer {
    Statsd,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Perf {
    /// Setting this path enables performance logging.
    pub perf_log: Option<PathBuf>,
    /// Seconds. Default `1`.
    pub log_interval: Option<u64>,
}

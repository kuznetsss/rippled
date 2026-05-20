use std::net::SocketAddr;
use serde::{Deserialize, Serialize};

use crate::types::path::RelPath;

/// Which insight server backend is in use. Currently only StatsD.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InsightServer {
    StatsD,
}

/// Configuration for the `[insight]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InsightConfig {
    pub server: InsightServer,
    /// Address of the stats sink.
    pub address: Option<SocketAddr>,
    /// Metric name prefix.
    pub prefix: Option<String>,
}

impl Default for InsightConfig {
    fn default() -> Self {
        InsightConfig {
            server: InsightServer::StatsD,
            address: None,
            prefix: None,
        }
    }
}

/// Configuration for the `[perf]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerfConfig {
    /// Path to the performance log file, resolved relative to the config dir.
    /// If `None`, performance logging is disabled.
    pub perf_log: Option<RelPath>,
    /// How often (in seconds) to flush performance metrics. Default 1.
    pub log_interval: u32,
}

impl Default for PerfConfig {
    fn default() -> Self {
        PerfConfig {
            perf_log: None,
            log_interval: 1,
        }
    }
}

//! `[insight]` and `[perf]` tables.

use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

use crate::ffi;

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Insight {
    /// Currently only `"statsd"` is recognized; omit the section to use the
    /// null collector.
    // FFI: `Insight::server()` below.
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

// ---- FFI projection types ----

impl From<InsightServer> for ffi::InsightServer {
    fn from(v: InsightServer) -> ffi::InsightServer {
        match v {
            InsightServer::Statsd => ffi::InsightServer::Statsd,
        }
    }
}

pub struct OptionalInsightServer(Option<InsightServer>);

impl From<Option<InsightServer>> for OptionalInsightServer {
    fn from(v: Option<InsightServer>) -> Self {
        Self(v)
    }
}

impl OptionalInsightServer {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::InsightServer, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalInsightServer has no value".into())
    }
}

// ---- Inherent getters on schema types ----

impl Insight {
    pub fn server(&self) -> Box<OptionalInsightServer> {
        Box::new(self.server.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ffi::InsightServer;

    fn ok_outcome(s: &str) -> Box<crate::schema::Config> {
        let (cfg, _) = crate::parse_from_str(s, crate::ConfigFormat::Toml)
            .expect("parse succeeded")
            .finalize()
            .expect("finalize succeeded");
        Box::new(cfg)
    }

    #[test]
    fn insight_server_present_and_absent() {
        let cfg = ok_outcome(
            r#"
                [insight]
                server = "statsd"
            "#,
        );
        assert!(matches!(
            cfg.insight().unwrap().server().value().unwrap(),
            InsightServer::Statsd
        ));

        let cfg = ok_outcome(
            r#"
                [insight]
                prefix = "x"
            "#,
        );
        assert!(!cfg.insight().unwrap().server().has_value());
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Perf {
    /// Setting this path enables performance logging.
    pub perf_log: Option<PathBuf>,
    /// Seconds. Default `1`.
    pub log_interval: Option<u64>,
}

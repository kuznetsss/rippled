use serde::{Deserialize, Serialize};

/// The `[crawl]` section — dual-shape in INI (legacy bool or kv map),
/// kv map only in TOML.
///
/// `LegacyBool` is only produced by the INI parser for historical configs that
/// use a single `true`/`false` bare line. TOML only allows `Detailed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CrawlConfig {
    /// A single bare boolean line (INI-only legacy form).
    LegacyBool(bool),
    /// Full kv-map form (valid in both INI and TOML).
    Detailed {
        #[serde(default)]
        overlay: bool,
        #[serde(default)]
        server: bool,
        #[serde(default)]
        counts: bool,
        #[serde(default)]
        unl: bool,
    },
}

impl Default for CrawlConfig {
    fn default() -> Self {
        CrawlConfig::Detailed {
            overlay: false,
            server: false,
            counts: false,
            unl: false,
        }
    }
}

/// The `[vl]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VlConfig {
    pub enabled: bool,
}

impl Default for VlConfig {
    fn default() -> Self {
        VlConfig { enabled: false }
    }
}

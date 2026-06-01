//! Polymorphic scalars that accept either a named alias or a numeric value.
//!
//! TOML is case-sensitive: only the lowercase canonical forms are accepted at
//! deserialization time (per §7.4 #2 of `config_schema.md`).

use crate::ffi;
use serde::{Deserialize, Serialize};

/// Startup mode, derived from CLI flags by [`Config::apply_cli_flags`].
///
/// This type is NOT read from the config file — it is populated from the
/// command line only and therefore marked `#[serde(skip)]` on `Config`.
/// Mirrors `StartUpType` in `src/xrpld/app/main/Main.cpp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartUpType {
    /// Normal startup (default).
    #[default]
    Normal,
    /// `--start`: start with a fresh empty ledger.
    Fresh,
    /// `--load` / `fast_load` / `--ledger` (without `--replay`): load a ledger.
    Load,
    /// `--ledgerfile`: load ledger from a file.
    LoadFile,
    /// `--ledger --replay`: replay the specified ledger.
    Replay,
    /// `--net`: network startup mode.
    Network,
}

/// `ledger_history`: integer count, or `"full"` / `"none"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LedgerHistory {
    Named(LedgerHistoryName),
    Numeric(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LedgerHistoryName {
    Full,
    None,
}

impl From<LedgerHistoryName> for ffi::LedgerHistoryKind {
    fn from(value: LedgerHistoryName) -> Self {
        match value {
            LedgerHistoryName::Full => ffi::LedgerHistoryKind::Full,
            LedgerHistoryName::None => ffi::LedgerHistoryKind::None,
        }
    }
}

/// `fetch_depth`: integer count, or `"full"` / `"none"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FetchDepth {
    Named(FetchDepthName),
    Numeric(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FetchDepthName {
    Full,
    None,
}

impl From<FetchDepthName> for ffi::FetchDepthKind {
    fn from(value: FetchDepthName) -> Self {
        match value {
            FetchDepthName::Full => ffi::FetchDepthKind::Full,
            FetchDepthName::None => ffi::FetchDepthKind::None,
        }
    }
}

/// `network_id`: integer in `[0, u32::MAX]`, or one of the well-known names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NetworkId {
    Named(NetworkIdName),
    Numeric(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkIdName {
    Main,
    Testnet,
    Devnet,
}

impl From<NetworkIdName> for ffi::NetworkIdKind {
    fn from(value: NetworkIdName) -> Self {
        match value {
            NetworkIdName::Main => ffi::NetworkIdKind::Main,
            NetworkIdName::Testnet => ffi::NetworkIdKind::Testnet,
            NetworkIdName::Devnet => ffi::NetworkIdKind::Devnet,
        }
    }
}

/// `node_size`: integer in `0..=4`, or one of the named tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NodeSize {
    Named(NodeSizeName),
    Numeric(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeSizeName {
    Tiny,
    Small,
    Medium,
    Large,
    Huge,
}

impl From<NodeSizeName> for ffi::NodeSizeKind {
    fn from(value: NodeSizeName) -> Self {
        match value {
            NodeSizeName::Tiny => ffi::NodeSizeKind::Tiny,
            NodeSizeName::Small => ffi::NodeSizeKind::Small,
            NodeSizeName::Medium => ffi::NodeSizeKind::Medium,
            NodeSizeName::Large => ffi::NodeSizeKind::Large,
            NodeSizeName::Huge => ffi::NodeSizeKind::Huge,
        }
    }
}

/// `relay_proposals` / `relay_validations` policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayMode {
    All,
    Trusted,
    DropUntrusted,
}

// ---- FFI projection types ----
//
// These live next to the schema types they wrap, imported into `ffi.rs`'s
// scope so cxx-bridge can resolve `super::OptionalT`.

impl From<RelayMode> for ffi::RelayMode {
    fn from(v: RelayMode) -> ffi::RelayMode {
        match v {
            RelayMode::All => ffi::RelayMode::All,
            RelayMode::Trusted => ffi::RelayMode::Trusted,
            RelayMode::DropUntrusted => ffi::RelayMode::DropUntrusted,
        }
    }
}

pub struct OptionalRelayMode(Option<RelayMode>);

impl From<Option<RelayMode>> for OptionalRelayMode {
    fn from(v: Option<RelayMode>) -> Self {
        Self(v)
    }
}

impl OptionalRelayMode {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::RelayMode, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalRelayMode has no value".into())
    }
}

pub struct OptionalLedgerHistory(Option<LedgerHistory>);

impl From<Option<LedgerHistory>> for OptionalLedgerHistory {
    fn from(v: Option<LedgerHistory>) -> Self {
        Self(v)
    }
}

impl OptionalLedgerHistory {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::LedgerHistoryKind, String> {
        match self.0 {
            Some(LedgerHistory::Named(name)) => Ok(name.into()),
            Some(LedgerHistory::Numeric(_)) => Ok(ffi::LedgerHistoryKind::Numeric),
            None => Err("OptionalLedgerHistory has no value".into()),
        }
    }

    pub fn numeric_value(&self) -> Result<u32, String> {
        match self.0 {
            Some(LedgerHistory::Numeric(n)) => Ok(n),
            Some(_) => Err("OptionalLedgerHistory is not Numeric".into()),
            None => Err("OptionalLedgerHistory has no value".into()),
        }
    }
}

pub struct OptionalFetchDepth(Option<FetchDepth>);

impl From<Option<FetchDepth>> for OptionalFetchDepth {
    fn from(v: Option<FetchDepth>) -> Self {
        Self(v)
    }
}

impl OptionalFetchDepth {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::FetchDepthKind, String> {
        match self.0 {
            Some(FetchDepth::Named(name)) => Ok(name.into()),
            Some(FetchDepth::Numeric(_)) => Ok(ffi::FetchDepthKind::Numeric),
            None => Err("OptionalFetchDepth has no value".into()),
        }
    }

    pub fn numeric_value(&self) -> Result<u32, String> {
        match self.0 {
            Some(FetchDepth::Numeric(n)) => Ok(n),
            Some(_) => Err("OptionalFetchDepth is not Numeric".into()),
            None => Err("OptionalFetchDepth has no value".into()),
        }
    }
}

pub struct OptionalNetworkId(Option<NetworkId>);

impl From<Option<NetworkId>> for OptionalNetworkId {
    fn from(v: Option<NetworkId>) -> Self {
        Self(v)
    }
}

impl OptionalNetworkId {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::NetworkIdKind, String> {
        match self.0 {
            Some(NetworkId::Named(name)) => Ok(name.into()),
            Some(NetworkId::Numeric(_)) => Ok(ffi::NetworkIdKind::Numeric),
            None => Err("OptionalNetworkId has no value".into()),
        }
    }

    pub fn numeric_value(&self) -> Result<u32, String> {
        match self.0 {
            Some(NetworkId::Numeric(n)) => Ok(n),
            Some(_) => Err("OptionalNetworkId is not Numeric".into()),
            None => Err("OptionalNetworkId has no value".into()),
        }
    }
}

pub struct OptionalNodeSize(Option<NodeSize>);

impl From<Option<NodeSize>> for OptionalNodeSize {
    fn from(v: Option<NodeSize>) -> Self {
        Self(v)
    }
}

impl OptionalNodeSize {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::NodeSizeKind, String> {
        match self.0 {
            Some(NodeSize::Named(name)) => Ok(name.into()),
            Some(NodeSize::Numeric(_)) => Ok(ffi::NodeSizeKind::Numeric),
            None => Err("OptionalNodeSize has no value".into()),
        }
    }

    pub fn numeric_value(&self) -> Result<u8, String> {
        match self.0 {
            Some(NodeSize::Numeric(n)) => Ok(n),
            Some(_) => Err("OptionalNodeSize is not Numeric".into()),
            None => Err("OptionalNodeSize has no value".into()),
        }
    }
}

// ---- Config getters (hand-written; fields are marked #[config_entry(skip)]) ----

impl super::Config {
    pub fn relay_proposals(&self) -> Box<OptionalRelayMode> {
        Box::new(self.relay_proposals.into())
    }

    pub fn relay_validations(&self) -> Box<OptionalRelayMode> {
        Box::new(self.relay_validations.into())
    }

    pub fn ledger_history(&self) -> Box<OptionalLedgerHistory> {
        Box::new(self.ledger_history.into())
    }

    pub fn fetch_depth(&self) -> Box<OptionalFetchDepth> {
        Box::new(self.fetch_depth.into())
    }

    pub fn network_id(&self) -> Box<OptionalNetworkId> {
        Box::new(self.network_id.into())
    }

    pub fn node_size(&self) -> Box<OptionalNodeSize> {
        Box::new(self.node_size.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ffi::{FetchDepthKind, LedgerHistoryKind, NetworkIdKind, NodeSizeKind, RelayMode};

    fn ok_outcome(s: &str) -> Box<crate::schema::Config> {
        let (cfg, _) = crate::parse_from_str(s, crate::ConfigFormat::Toml)
            .expect("parse succeeded")
            .finalize()
            .expect("finalize succeeded");
        Box::new(cfg)
    }

    // ----- Data-less optional-enum wrappers -----

    #[test]
    fn relay_proposals_present_returns_variant() {
        let cfg = ok_outcome(r#"relay_proposals = "trusted""#);
        let v = cfg.relay_proposals();
        assert!(v.has_value());
        assert!(matches!(v.value().unwrap(), RelayMode::Trusted));
    }

    #[test]
    fn relay_proposals_absent_value_throws() {
        let cfg = ok_outcome("");
        let v = cfg.relay_proposals();
        assert!(!v.has_value());
        assert!(v.value().is_err());
    }

    #[test]
    fn relay_validations_uses_same_optional_relay_mode() {
        let cfg = ok_outcome(r#"relay_validations = "drop_untrusted""#);
        assert!(matches!(
            cfg.relay_validations().value().unwrap(),
            RelayMode::DropUntrusted
        ));
    }

    // ----- Polymorphic wrappers -----

    #[test]
    fn ledger_history_named_full() {
        let cfg = ok_outcome(r#"ledger_history = "full""#);
        let h = cfg.ledger_history();
        assert!(h.has_value());
        assert!(matches!(h.kind().unwrap(), LedgerHistoryKind::Full));
        // numeric_value throws because the kind is not Numeric.
        assert!(h.numeric_value().is_err());
    }

    #[test]
    fn ledger_history_named_none() {
        let cfg = ok_outcome(r#"ledger_history = "none""#);
        assert!(matches!(
            cfg.ledger_history().kind().unwrap(),
            LedgerHistoryKind::None
        ));
    }

    #[test]
    fn ledger_history_numeric() {
        let cfg = ok_outcome("ledger_history = 100000");
        let h = cfg.ledger_history();
        assert!(matches!(h.kind().unwrap(), LedgerHistoryKind::Numeric));
        assert_eq!(h.numeric_value().unwrap(), 100_000);
    }

    #[test]
    fn ledger_history_absent_throws_on_kind() {
        let cfg = ok_outcome("");
        let h = cfg.ledger_history();
        assert!(!h.has_value());
        assert!(h.kind().is_err());
        assert!(h.numeric_value().is_err());
    }

    #[test]
    fn fetch_depth_named_and_numeric() {
        let cfg = ok_outcome(r#"fetch_depth = "full""#);
        assert!(matches!(
            cfg.fetch_depth().kind().unwrap(),
            FetchDepthKind::Full
        ));

        let cfg = ok_outcome("fetch_depth = 5000");
        let fd = cfg.fetch_depth();
        assert!(matches!(fd.kind().unwrap(), FetchDepthKind::Numeric));
        assert_eq!(fd.numeric_value().unwrap(), 5000);
    }

    #[test]
    fn network_id_named_main() {
        let cfg = ok_outcome(r#"network_id = "main""#);
        assert!(matches!(
            cfg.network_id().kind().unwrap(),
            NetworkIdKind::Main
        ));
        assert!(cfg.network_id().numeric_value().is_err());
    }

    #[test]
    fn network_id_named_testnet() {
        let cfg = ok_outcome(r#"network_id = "testnet""#);
        assert!(matches!(
            cfg.network_id().kind().unwrap(),
            NetworkIdKind::Testnet
        ));
    }

    #[test]
    fn network_id_named_devnet() {
        let cfg = ok_outcome(r#"network_id = "devnet""#);
        assert!(matches!(
            cfg.network_id().kind().unwrap(),
            NetworkIdKind::Devnet
        ));
    }

    #[test]
    fn network_id_numeric() {
        let cfg = ok_outcome("network_id = 1234");
        let n = cfg.network_id();
        assert!(matches!(n.kind().unwrap(), NetworkIdKind::Numeric));
        assert_eq!(n.numeric_value().unwrap(), 1234);
    }

    #[test]
    fn node_size_named_huge() {
        let cfg = ok_outcome(r#"node_size = "huge""#);
        let n = cfg.node_size();
        assert!(matches!(n.kind().unwrap(), NodeSizeKind::Huge));
        assert!(n.numeric_value().is_err());
    }

    #[test]
    fn node_size_numeric() {
        let cfg = ok_outcome("node_size = 3");
        let n = cfg.node_size();
        assert!(matches!(n.kind().unwrap(), NodeSizeKind::Numeric));
        assert_eq!(n.numeric_value().unwrap(), 3u8);
    }

    #[test]
    fn node_size_absent() {
        let cfg = ok_outcome("");
        let n = cfg.node_size();
        assert!(!n.has_value());
        assert!(n.kind().is_err());
    }
}

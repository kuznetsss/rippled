//! Polymorphic scalars that accept either a named alias or a numeric value.
//!
//! TOML is case-sensitive: only the lowercase canonical forms are accepted at
//! deserialization time (per §7.4 #2 of `config_schema.md`).

use serde::{Deserialize, Serialize};

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

/// `relay_proposals` / `relay_validations` policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayMode {
    All,
    Trusted,
    DropUntrusted,
}

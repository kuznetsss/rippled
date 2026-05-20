use serde::{Deserialize, Serialize};

/// A trusted validator entry from `[validators]` or `[validator_keys]`.
/// Both sections feed the same `trusted_validators: Vec<TrustedValidator>` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustedValidator {
    /// Base58 public key.
    pub key: String,
    /// Optional human-readable label.
    pub label: Option<String>,
}

/// An entry from `[cluster_nodes]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterNode {
    /// Base58 public key.
    pub key: String,
    /// Optional human-readable label.
    pub label: Option<String>,
}

/// An entry from `[amendments]` or `[veto_amendments]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownAmendment {
    /// 32-byte amendment ID, decoded from the 64-hex string in the config.
    pub id: [u8; 32],
    /// Human-readable amendment name.
    pub name: String,
}

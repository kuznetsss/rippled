use serde::{Deserialize, Serialize};

/// Fee schedule — amounts that the node votes to use during fee-setting ledgers.
/// Defaults match the C++ `FeeSetup` defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VotingConfig {
    /// Cost of a reference transaction in drops. Default 10.
    pub reference_fee: u64,
    /// Per-account reserve requirement in drops. Default 1_000_000 (1 XRP).
    pub account_reserve: u64,
    /// Per-owned-item reserve requirement in drops. Default 200_000 (0.2 XRP).
    pub owner_reserve: u64,
}

impl Default for VotingConfig {
    fn default() -> Self {
        VotingConfig {
            reference_fee: 10,
            account_reserve: 1_000_000,
            owner_reserve: 200_000,
        }
    }
}

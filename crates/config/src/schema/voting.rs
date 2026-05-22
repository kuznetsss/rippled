//! `[voting]` table.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Voting {
    /// Drops. Default `10`. Top-level `fee_default` overrides this post-load.
    pub reference_fee: Option<u64>,
    /// Drops. Default `1_000_000` (1 XRP).
    pub account_reserve: Option<u32>,
    /// Drops. Default `200_000` (0.2 XRP).
    pub owner_reserve: Option<u32>,
}

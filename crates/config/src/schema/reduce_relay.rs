//! `[reduce_relay]` table.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReduceRelay {
    /// Mutually exclusive with [`Self::vp_enable`].
    pub vp_base_squelch_enable: Option<bool>,
    /// Deprecated alias of [`Self::vp_base_squelch_enable`].
    pub vp_enable: Option<bool>,
    /// Must be `>= 3`. Default `5`.
    pub vp_base_squelch_max_selected_peers: Option<u32>,
    pub tx_enable: Option<bool>,
    pub tx_metrics: Option<bool>,
    /// Must be `>= 10`. Default `20`.
    pub tx_min_peers: Option<u32>,
    /// Range `[10, 100]`. Default `25`.
    pub tx_relay_percentage: Option<u32>,
}

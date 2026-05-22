//! `[overlay]` table.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    /// Publicly reachable IP. Must not be a private address.
    pub public_ip: Option<String>,
    /// Upper bound on inbound peer connections. `>= 0`; auto when unset.
    pub ip_limit: Option<i32>,
    /// Seconds. Range `[300, 1800]`. Default `600`.
    pub max_unknown_time: Option<u32>,
    /// Seconds. Range `[60, 900]`. Default `300`.
    pub max_diverged_time: Option<u32>,
}

//! `[hashrouter]` table.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HashRouter {
    /// Seconds. Must be `>= 12`.
    pub hold_time: Option<i32>,
    /// Seconds. Must be `>= 8` and `<= hold_time`.
    pub relay_time: Option<i32>,
}

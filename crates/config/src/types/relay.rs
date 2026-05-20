use serde::{Deserialize, Serialize};

/// Policy for relaying unvalidated proposals/validations from untrusted peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayPolicy {
    All,
    Trusted,
    DropUntrusted,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        RelayPolicy::Trusted
    }
}

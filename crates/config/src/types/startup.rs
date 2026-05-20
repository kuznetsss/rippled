use serde::{Deserialize, Serialize};

/// How the node should start up. Mirrors `Config::StartUpType` from the C++.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartUpType {
    Normal,
    Load,
    Replay,
    NewChain,
    FromLedger,
}

impl Default for StartUpType {
    fn default() -> Self {
        StartUpType::Normal
    }
}

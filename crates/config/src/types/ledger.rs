use serde::{Deserialize, Serialize};

/// How much ledger history to keep. `Full` = `UINT32_MAX`, `None_` = 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerHistory {
    /// Keep the full history (`UINT32_MAX`).
    Full,
    /// Keep no history (0).
    None_,
    /// Keep this many ledgers.
    Count(u32),
}

impl Default for LedgerHistory {
    fn default() -> Self {
        LedgerHistory::Count(256)
    }
}

/// How deep to fetch ledger data. `Full` = `UINT32_MAX`, `None_` = 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FetchDepth {
    /// Fetch full depth (`UINT32_MAX`).
    Full,
    /// Do not fetch (0).
    None_,
    /// Fetch this many levels.
    Count(u32),
}

impl Default for FetchDepth {
    fn default() -> Self {
        FetchDepth::Count(1_000_000_000)
    }
}

/// Configuration for the ledger transaction tables.
///
/// Canonical home of the `USE_TX_TABLES_` flag (previously lifted directly
/// onto the C++ `Config` class from `[ledger_tx_tables]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct LedgerTxTablesConfig {
    pub use_tx_tables: bool,
}

impl Default for LedgerTxTablesConfig {
    fn default() -> Self {
        LedgerTxTablesConfig {
            use_tx_tables: true,
        }
    }
}

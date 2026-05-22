//! Database-related sections: `[node_db]`, `[import_db]`, `[sqlite]`,
//! `[sqdb]`, `[ledger_tx_tables]`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// `[node_db]` / `[import_db]` — common shape, tagged by backend `type`.
///
/// `nudb_block_size` is only meaningful for `NuDB`; `cache_mb` and
/// `filter_bits` are only meaningful for `RocksDB`. The serde tag rejects
/// unrelated keys at deserialization.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum NodeDb {
    #[serde(rename = "NuDB")]
    NuDb(NuDbOptions),
    #[serde(rename = "RocksDB")]
    RocksDb(RocksDbOptions),
}

/// Fields shared by every backend.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NodeDbCommon {
    pub path: PathBuf,
    /// When `true`, also forces `START_UP = Load` at startup.
    pub fast_load: Option<bool>,
    /// Default `kXRP_LEDGER_EARLIEST_SEQ` (32570). Must be `>= 1`.
    pub earliest_seq: Option<u32>,
    /// `0` (default) disables online delete. Non-zero values must be
    /// `>= kMINIMUM_DELETION_INTERVAL` and `>= ledger_history`.
    pub online_delete: Option<u32>,
    /// Only meaningful when `online_delete` is set.
    pub advisory_delete: Option<bool>,
    /// Only meaningful when `online_delete` is set. Default `100`.
    pub delete_batch: Option<u32>,
    /// Default `100`. Legacy alias `backOff` is INI-only.
    pub back_off_milliseconds: Option<u32>,
    /// Default `60`.
    pub age_threshold_seconds: Option<u32>,
    /// Default `5`.
    pub recovery_wait_seconds: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NuDbOptions {
    #[serde(flatten)]
    pub common: NodeDbCommon,
    /// Power of two in `[4096, 32768]`. Default `4096`.
    pub nudb_block_size: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RocksDbOptions {
    #[serde(flatten)]
    pub common: NodeDbCommon,
    /// Derived from `SizedItem::HashNodeDbCache` when absent.
    pub cache_mb: Option<u32>,
    /// Default `10` (only when `NODE_SIZE >= 2`).
    pub filter_bits: Option<u32>,
}

/// `[sqlite]` table.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sqlite {
    /// `"high"` or `"low"`. Cannot coexist with `journal_mode`,
    /// `synchronous`, or `temp_store` (validated post-deserialize).
    pub safety_level: Option<SafetyLevel>,
    pub journal_mode: Option<JournalMode>,
    pub synchronous: Option<Synchronous>,
    pub temp_store: Option<TempStore>,
    /// Power of two in `[512, 65536]`. Default `4096`.
    pub page_size: Option<u32>,
    /// Default `1582080`.
    pub journal_size_limit: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SafetyLevel {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalMode {
    Delete,
    Truncate,
    Persist,
    Memory,
    Wal,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Synchronous {
    Off,
    Normal,
    Full,
    Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TempStore {
    Default,
    File,
    Memory,
}

/// `[sqdb]` — SOCI backend selector.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sqdb {
    /// Only `"sqlite"` is accepted.
    pub backend: Option<SqdbBackend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SqdbBackend {
    Sqlite,
}

/// `[ledger_tx_tables]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerTxTables {
    /// Default `true`.
    pub use_tx_tables: Option<bool>,
}

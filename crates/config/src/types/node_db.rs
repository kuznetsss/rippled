use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// The backend engine for the node database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeDbKind {
    NuDb,
    RocksDb,
}

impl Default for NodeDbKind {
    fn default() -> Self {
        NodeDbKind::NuDb
    }
}

/// Configuration for a node database (`[node_db]` or `[import_db]`).
/// Both sections share the same schema.
///
/// RocksDB-specific tunables that Config does not interpret go into
/// `backend_extras` and are forwarded to `NodeStore::Manager` verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeDbConfig {
    pub kind: NodeDbKind,
    /// Filesystem path to the database directory. Not auto-resolved (analysis §6.6).
    pub path: PathBuf,
    /// Canonical home of the `FAST_LOAD` flag (previously lifted onto C++ `Config`).
    pub fast_load: bool,
    /// Earliest ledger sequence stored. Must be >= 1. Default 32570.
    pub earliest_seq: u32,
    /// Online-delete threshold in ledgers. Must be >= 256 when set, and >= ledger_history.
    pub online_delete: Option<u32>,
    /// Whether the node uses advisory deletion instead of automatic deletion.
    pub advisory_delete: bool,
    /// Number of ledgers to delete per batch. Default 100.
    pub delete_batch: u32,
    /// Milliseconds to back off between deletion batches. Default 100.
    pub back_off_milliseconds: u32,
    /// Age threshold for deletion in seconds. Default 60.
    pub age_threshold_seconds: u32,
    /// Seconds to wait before retrying after a recovery. Default 5.
    pub recovery_wait_seconds: u32,
    /// NuDB block size in bytes. Must be a power of 2 in 4096..=32768. Default 4096.
    pub nudb_block_size: u32,
    /// RocksDB-specific tunables, passed through to NodeStore unchanged.
    /// In INI: any unknown key under `[node_db]` falls here.
    /// In TOML: only explicit `[node_db.extras]` sub-table keys.
    pub backend_extras: BTreeMap<String, String>,
}

impl Default for NodeDbConfig {
    fn default() -> Self {
        NodeDbConfig {
            kind: NodeDbKind::NuDb,
            path: PathBuf::new(),
            fast_load: false,
            earliest_seq: 32570,
            online_delete: None,
            advisory_delete: false,
            delete_batch: 100,
            back_off_milliseconds: 100,
            age_threshold_seconds: 60,
            recovery_wait_seconds: 5,
            nudb_block_size: 4096,
            backend_extras: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_db_default_values() {
        let c = NodeDbConfig::default();
        assert_eq!(c.kind, NodeDbKind::NuDb);
        assert!(!c.fast_load);
        assert_eq!(c.earliest_seq, 32570);
        assert_eq!(c.online_delete, None);
        assert!(!c.advisory_delete);
        assert_eq!(c.delete_batch, 100);
        assert_eq!(c.back_off_milliseconds, 100);
        assert_eq!(c.age_threshold_seconds, 60);
        assert_eq!(c.recovery_wait_seconds, 5);
        assert_eq!(c.nudb_block_size, 4096);
        assert!(c.backend_extras.is_empty());
    }

    #[test]
    fn node_db_kind_default_is_nudb() {
        assert_eq!(NodeDbKind::default(), NodeDbKind::NuDb);
    }

    #[test]
    fn node_db_default_passes_strict_validation() {
        NodeDbConfig::default()
            .validate_strict("node_db")
            .expect("default should be valid");
    }

    #[test]
    fn node_db_earliest_seq_min_boundary() {
        let mut c = NodeDbConfig::default();
        c.earliest_seq = 1;
        assert!(c.validate_strict("node_db").is_ok());
    }

    #[test]
    fn node_db_earliest_seq_zero_fails() {
        let mut c = NodeDbConfig::default();
        c.earliest_seq = 0;
        let err = c.validate_strict("node_db").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("earliest_seq"), "got: {msg}");
    }

    #[test]
    fn node_db_nudb_block_size_valid_4096() {
        let mut c = NodeDbConfig::default();
        c.nudb_block_size = 4096;
        assert!(c.validate_strict("node_db").is_ok());
    }

    #[test]
    fn node_db_nudb_block_size_valid_32768() {
        let mut c = NodeDbConfig::default();
        c.nudb_block_size = 32768;
        assert!(c.validate_strict("node_db").is_ok());
    }

    #[test]
    fn node_db_nudb_block_size_too_small() {
        let mut c = NodeDbConfig::default();
        c.nudb_block_size = 2048;
        let err = c.validate_strict("node_db").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nudb_block_size"), "got: {msg}");
    }

    #[test]
    fn node_db_nudb_block_size_too_large() {
        let mut c = NodeDbConfig::default();
        c.nudb_block_size = 65536;
        assert!(c.validate_strict("node_db").is_err());
    }

    #[test]
    fn node_db_nudb_block_size_not_power_of_two() {
        let mut c = NodeDbConfig::default();
        c.nudb_block_size = 5000; // in range but not power of 2
        assert!(c.validate_strict("node_db").is_err());
    }

    #[test]
    fn node_db_online_delete_valid_256() {
        let mut c = NodeDbConfig::default();
        c.online_delete = Some(256);
        assert!(c.validate_strict("node_db").is_ok());
    }

    #[test]
    fn node_db_online_delete_too_low() {
        let mut c = NodeDbConfig::default();
        c.online_delete = Some(255);
        let err = c.validate_strict("node_db").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("online_delete"), "got: {msg}");
    }

    #[test]
    fn node_db_online_delete_none_ok() {
        let mut c = NodeDbConfig::default();
        c.online_delete = None;
        assert!(c.validate_strict("node_db").is_ok());
    }

    #[test]
    fn node_db_section_name_in_error() {
        let mut c = NodeDbConfig::default();
        c.earliest_seq = 0;
        let err = c.validate_strict("import_db").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("import_db"), "got: {msg}");
    }
}

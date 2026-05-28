//! Database-related sections: `[node_db]`, `[import_db]`, `[sqlite]`,
//! `[sqdb]`, `[ledger_tx_tables]`.

use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

use crate::ffi;
use crate::ffi::{OptionalBool, OptionalU32};

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
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Sqlite {
    /// `"high"` or `"low"`. Cannot coexist with `journal_mode`,
    /// `synchronous`, or `temp_store` (validated post-deserialize).
    // FFI: `Sqlite::safety_level()` below.
    #[config_entry(skip)]
    pub safety_level: Option<SafetyLevel>,
    // FFI: `Sqlite::journal_mode()` below.
    #[config_entry(skip)]
    pub journal_mode: Option<JournalMode>,
    // FFI: `Sqlite::synchronous()` below.
    #[config_entry(skip)]
    pub synchronous: Option<Synchronous>,
    // FFI: `Sqlite::temp_store()` below.
    #[config_entry(skip)]
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
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Sqdb {
    /// Only `"sqlite"` is accepted.
    // FFI: `Sqdb::backend()` below.
    #[config_entry(skip)]
    pub backend: Option<SqdbBackend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SqdbBackend {
    Sqlite,
}

/// `[ledger_tx_tables]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct LedgerTxTables {
    /// Default `true`.
    pub use_tx_tables: Option<bool>,
}

// ---- FFI projection types ----
//
// These live next to the schema types they wrap, imported into `ffi.rs`'s
// scope so cxx-bridge can resolve `super::OptionalT`.

impl From<SafetyLevel> for ffi::SafetyLevel {
    fn from(v: SafetyLevel) -> ffi::SafetyLevel {
        match v {
            SafetyLevel::High => ffi::SafetyLevel::High,
            SafetyLevel::Low => ffi::SafetyLevel::Low,
        }
    }
}

pub struct OptionalSafetyLevel(Option<SafetyLevel>);

impl From<Option<SafetyLevel>> for OptionalSafetyLevel {
    fn from(v: Option<SafetyLevel>) -> Self {
        Self(v)
    }
}

impl OptionalSafetyLevel {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::SafetyLevel, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalSafetyLevel has no value".into())
    }
}

impl From<JournalMode> for ffi::JournalMode {
    fn from(v: JournalMode) -> ffi::JournalMode {
        match v {
            JournalMode::Delete => ffi::JournalMode::Delete,
            JournalMode::Truncate => ffi::JournalMode::Truncate,
            JournalMode::Persist => ffi::JournalMode::Persist,
            JournalMode::Memory => ffi::JournalMode::Memory,
            JournalMode::Wal => ffi::JournalMode::Wal,
            JournalMode::Off => ffi::JournalMode::Off,
        }
    }
}

pub struct OptionalJournalMode(Option<JournalMode>);

impl From<Option<JournalMode>> for OptionalJournalMode {
    fn from(v: Option<JournalMode>) -> Self {
        Self(v)
    }
}

impl OptionalJournalMode {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::JournalMode, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalJournalMode has no value".into())
    }
}

impl From<Synchronous> for ffi::Synchronous {
    fn from(v: Synchronous) -> ffi::Synchronous {
        match v {
            Synchronous::Off => ffi::Synchronous::Off,
            Synchronous::Normal => ffi::Synchronous::Normal,
            Synchronous::Full => ffi::Synchronous::Full,
            Synchronous::Extra => ffi::Synchronous::Extra,
        }
    }
}

pub struct OptionalSynchronous(Option<Synchronous>);

impl From<Option<Synchronous>> for OptionalSynchronous {
    fn from(v: Option<Synchronous>) -> Self {
        Self(v)
    }
}

impl OptionalSynchronous {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::Synchronous, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalSynchronous has no value".into())
    }
}

impl From<TempStore> for ffi::TempStore {
    fn from(v: TempStore) -> ffi::TempStore {
        match v {
            TempStore::Default => ffi::TempStore::Default,
            TempStore::File => ffi::TempStore::File,
            TempStore::Memory => ffi::TempStore::Memory,
        }
    }
}

pub struct OptionalTempStore(Option<TempStore>);

impl From<Option<TempStore>> for OptionalTempStore {
    fn from(v: Option<TempStore>) -> Self {
        Self(v)
    }
}

impl OptionalTempStore {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::TempStore, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalTempStore has no value".into())
    }
}

impl From<&NodeDb> for ffi::NodeDbKind {
    fn from(value: &NodeDb) -> Self {
        match value {
            NodeDb::NuDb(_) => ffi::NodeDbKind::NuDb,
            NodeDb::RocksDb(_) => ffi::NodeDbKind::RocksDb,
        }
    }
}

impl From<SqdbBackend> for ffi::SqdbBackend {
    fn from(v: SqdbBackend) -> ffi::SqdbBackend {
        match v {
            SqdbBackend::Sqlite => ffi::SqdbBackend::Sqlite,
        }
    }
}

pub struct OptionalSqdbBackend(Option<SqdbBackend>);

impl From<Option<SqdbBackend>> for OptionalSqdbBackend {
    fn from(v: Option<SqdbBackend>) -> Self {
        Self(v)
    }
}

impl OptionalSqdbBackend {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<ffi::SqdbBackend, String> {
        self.0
            .map(Into::into)
            .ok_or_else(|| "OptionalSqdbBackend has no value".into())
    }
}

pub struct OptionalNodeDb(Option<NodeDb>);

impl From<Option<NodeDb>> for OptionalNodeDb {
    fn from(v: Option<NodeDb>) -> Self {
        Self(v)
    }
}

impl OptionalNodeDb {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::NodeDbKind, String> {
        match &self.0 {
            Some(node_db) => Ok(node_db.into()),
            None => Err("OptionalNodeDb has no value".into()),
        }
    }

    fn common(&self) -> Option<&NodeDbCommon> {
        match &self.0 {
            Some(NodeDb::NuDb(o)) => Some(&o.common),
            Some(NodeDb::RocksDb(o)) => Some(&o.common),
            None => None,
        }
    }

    pub fn path(&self) -> Result<String, String> {
        self.common()
            .map(|c| c.path.to_string_lossy().into_owned())
            .ok_or_else(|| "OptionalNodeDb has no value".into())
    }

    pub fn fast_load(&self) -> Box<OptionalBool> {
        Box::new(self.common().and_then(|c| c.fast_load).into())
    }

    pub fn earliest_seq(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.earliest_seq).into())
    }

    pub fn online_delete(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.online_delete).into())
    }

    pub fn advisory_delete(&self) -> Box<OptionalBool> {
        Box::new(self.common().and_then(|c| c.advisory_delete).into())
    }

    pub fn delete_batch(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.delete_batch).into())
    }

    pub fn back_off_milliseconds(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.back_off_milliseconds).into())
    }

    pub fn age_threshold_seconds(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.age_threshold_seconds).into())
    }

    pub fn recovery_wait_seconds(&self) -> Box<OptionalU32> {
        Box::new(self.common().and_then(|c| c.recovery_wait_seconds).into())
    }

    pub fn nudb_block_size(&self) -> Box<OptionalU32> {
        let v = match &self.0 {
            Some(NodeDb::NuDb(o)) => o.nudb_block_size,
            _ => None,
        };
        Box::new(v.into())
    }

    pub fn cache_mb(&self) -> Box<OptionalU32> {
        let v = match &self.0 {
            Some(NodeDb::RocksDb(o)) => o.cache_mb,
            _ => None,
        };
        Box::new(v.into())
    }

    pub fn filter_bits(&self) -> Box<OptionalU32> {
        let v = match &self.0 {
            Some(NodeDb::RocksDb(o)) => o.filter_bits,
            _ => None,
        };
        Box::new(v.into())
    }
}

// ---- Inherent getters on Config ----

impl super::Config {
    pub fn node_db(&self) -> Box<OptionalNodeDb> {
        Box::new(self.node_db.clone().into())
    }

    pub fn import_db(&self) -> Box<OptionalNodeDb> {
        Box::new(self.import_db.clone().into())
    }
}

// ---- Inherent getters on schema types ----

impl Sqlite {
    pub fn safety_level(&self) -> Box<OptionalSafetyLevel> {
        Box::new(self.safety_level.into())
    }

    pub fn journal_mode(&self) -> Box<OptionalJournalMode> {
        Box::new(self.journal_mode.into())
    }

    pub fn synchronous(&self) -> Box<OptionalSynchronous> {
        Box::new(self.synchronous.into())
    }

    pub fn temp_store(&self) -> Box<OptionalTempStore> {
        Box::new(self.temp_store.into())
    }
}

impl Sqdb {
    pub fn backend(&self) -> Box<OptionalSqdbBackend> {
        Box::new(self.backend.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ffi::{JournalMode, NodeDbKind, SafetyLevel, SqdbBackend, Synchronous, TempStore};

    fn ok_outcome(s: &str) -> Box<crate::schema::Config> {
        let (cfg, _) = crate::parse_from_str(s, crate::ConfigFormat::Toml, crate::LoadOptions::default())
            .expect("parse succeeded");
        Box::new(cfg)
    }

    // ----- Sqlite wrappers -----

    #[test]
    fn sqlite_safety_level_present() {
        let cfg = ok_outcome(
            r#"
                [sqlite]
                safety_level = "high"
            "#,
        );
        let s = cfg.sqlite().unwrap();
        assert!(s.safety_level().has_value());
        assert!(matches!(
            s.safety_level().value().unwrap(),
            SafetyLevel::High
        ));
    }

    #[test]
    fn sqlite_safety_level_absent() {
        let cfg = ok_outcome(
            r#"
                [sqlite]
                page_size = 4096
            "#,
        );
        let s = cfg.sqlite().unwrap();
        assert!(!s.safety_level().has_value());
        assert!(s.safety_level().value().is_err());
    }

    #[test]
    fn sqlite_journal_mode_all_variants_roundtrip() {
        // Each variant is matched explicitly to avoid the missing-Debug
        // problem with cxx-shared enums.
        fn check(name: &str, p: impl Fn(JournalMode) -> bool) {
            let cfg = ok_outcome(&format!(
                r#"
                    [sqlite]
                    journal_mode = "{name}"
                "#
            ));
            let s = cfg.sqlite().unwrap();
            assert!(p(s.journal_mode().value().unwrap()), "{name}");
        }
        check("delete", |v| matches!(v, JournalMode::Delete));
        check("truncate", |v| matches!(v, JournalMode::Truncate));
        check("persist", |v| matches!(v, JournalMode::Persist));
        check("memory", |v| matches!(v, JournalMode::Memory));
        check("wal", |v| matches!(v, JournalMode::Wal));
        check("off", |v| matches!(v, JournalMode::Off));
    }

    #[test]
    fn sqlite_synchronous_present() {
        let cfg = ok_outcome(
            r#"
                [sqlite]
                synchronous = "extra"
            "#,
        );
        assert!(matches!(
            cfg.sqlite().unwrap().synchronous().value().unwrap(),
            Synchronous::Extra
        ));
    }

    #[test]
    fn sqlite_temp_store_present_and_absent() {
        let cfg = ok_outcome(
            r#"
                [sqlite]
                temp_store = "memory"
            "#,
        );
        assert!(matches!(
            cfg.sqlite().unwrap().temp_store().value().unwrap(),
            TempStore::Memory
        ));

        let cfg = ok_outcome(
            r#"
                [sqlite]
                page_size = 4096
            "#,
        );
        assert!(!cfg.sqlite().unwrap().temp_store().has_value());
    }

    #[test]
    fn sqdb_backend_present_and_absent() {
        let cfg = ok_outcome(
            r#"
                [sqdb]
                backend = "sqlite"
            "#,
        );
        assert!(matches!(
            cfg.sqdb().unwrap().backend().value().unwrap(),
            SqdbBackend::Sqlite
        ));

        let cfg = ok_outcome("[sqdb]");
        assert!(!cfg.sqdb().unwrap().backend().has_value());
    }

    // ----- NodeDb tagged wrapper -----

    #[test]
    fn node_db_absent() {
        let cfg = ok_outcome("");
        let db = cfg.node_db();
        assert!(!db.has_value());
        assert!(db.kind().is_err());
        assert!(db.path().is_err());
        // Variant-specific fields are absent when has_value() is false.
        assert!(!db.nudb_block_size().has_value());
        assert!(!db.cache_mb().has_value());
        assert!(!db.filter_bits().has_value());
    }

    #[test]
    fn node_db_nudb_variant() {
        let cfg = ok_outcome(
            r#"
                [node_db]
                type            = "NuDB"
                path            = "/var/lib/xrpld/nudb"
                online_delete   = 2000
                nudb_block_size = 4096
            "#,
        );
        let db = cfg.node_db();
        assert!(db.has_value());
        assert!(matches!(db.kind().unwrap(), NodeDbKind::NuDb));
        assert_eq!(db.path().unwrap(), "/var/lib/xrpld/nudb");
        assert!(db.online_delete().has_value());
        assert_eq!(db.online_delete().value().unwrap(), 2000);
        assert!(db.nudb_block_size().has_value());
        assert_eq!(db.nudb_block_size().value().unwrap(), 4096);
        // RocksDB-only fields are silently absent — no throw.
        assert!(!db.cache_mb().has_value());
        assert!(!db.filter_bits().has_value());
    }

    #[test]
    fn node_db_rocksdb_variant() {
        let cfg = ok_outcome(
            r#"
                [node_db]
                type     = "RocksDB"
                path     = "/var/lib/xrpld/rocksdb"
                cache_mb = 512
            "#,
        );
        let db = cfg.node_db();
        assert!(matches!(db.kind().unwrap(), NodeDbKind::RocksDb));
        assert_eq!(db.path().unwrap(), "/var/lib/xrpld/rocksdb");
        assert!(db.cache_mb().has_value());
        assert_eq!(db.cache_mb().value().unwrap(), 512);
        // NuDB-only field is silently absent.
        assert!(!db.nudb_block_size().has_value());
    }

    #[test]
    fn node_db_common_fields_visible_on_either_variant() {
        let cfg = ok_outcome(
            r#"
                [node_db]
                type                 = "RocksDB"
                path                 = "/p"
                fast_load            = true
                earliest_seq         = 32570
                online_delete        = 2000
                advisory_delete      = true
                delete_batch         = 100
                back_off_milliseconds = 200
                age_threshold_seconds = 60
                recovery_wait_seconds = 5
            "#,
        );
        let db = cfg.node_db();
        assert!(db.fast_load().value().unwrap());
        assert_eq!(db.earliest_seq().value().unwrap(), 32570);
        assert_eq!(db.online_delete().value().unwrap(), 2000);
        assert!(db.advisory_delete().value().unwrap());
        assert_eq!(db.delete_batch().value().unwrap(), 100);
        assert_eq!(db.back_off_milliseconds().value().unwrap(), 200);
        assert_eq!(db.age_threshold_seconds().value().unwrap(), 60);
        assert_eq!(db.recovery_wait_seconds().value().unwrap(), 5);
    }

    #[test]
    fn import_db_uses_same_wrapper() {
        let cfg = ok_outcome(
            r#"
                [import_db]
                type = "NuDB"
                path = "/import"
            "#,
        );
        let db = cfg.import_db();
        assert!(matches!(db.kind().unwrap(), NodeDbKind::NuDb));
        assert_eq!(db.path().unwrap(), "/import");
        assert!(!cfg.node_db().has_value());
    }
}

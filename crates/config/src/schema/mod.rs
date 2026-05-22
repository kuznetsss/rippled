//! TOML-shaped schema for the `xrpld` configuration file.
//!
//! Mirrors `config_schema.md` §7. Every field is `Option` (or has a sensible
//! default) so a minimal TOML file deserializes cleanly. Cross-section
//! validation is intentionally not performed here — that belongs in the
//! loader layer.

use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

pub mod database;
pub mod diagnostics;
pub mod enums;
pub mod grpc;
pub mod hashrouter;
pub mod misc;
pub mod overlay;
pub mod reduce_relay;
pub mod server;
pub mod transaction_queue;
pub mod voting;

use database::{LedgerTxTables, NodeDb, Sqdb, Sqlite};
use diagnostics::{Insight, Perf};
use enums::{FetchDepth, LedgerHistory, NetworkId, NodeSize, RelayMode};
use grpc::Grpc;
use hashrouter::HashRouter;
use misc::{Crawl, Vl};
use overlay::Overlay;
use reduce_relay::ReduceRelay;
use server::Server;
use transaction_queue::TransactionQueue;
use voting::Voting;

/// Root configuration document.
///
/// Top-level keys carry the values that the legacy INI loader stored in
/// single-line or list-style sections; nested tables map one-to-one onto the
/// INI sections they replace.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Config {
    // ----- List-style top-level keys (legacy value-line sections) -----
    /// `[ips]` — bootstrap peers (`"host port"` or `"host"`).
    #[serde(default)]
    pub ips: Vec<String>,
    /// `[ips_fixed]` — sticky outbound peers (`"host port"`).
    #[serde(default)]
    pub ips_fixed: Vec<String>,
    /// `[validators]` — trusted validator public keys (`n…`).
    #[serde(default)]
    pub validators: Vec<String>,
    /// `[validator_keys]` — merged into `validators` post-load.
    #[serde(default)]
    pub validator_keys: Vec<String>,
    /// `[validator_list_sites]` — validator-list publisher URIs.
    #[serde(default)]
    pub validator_list_sites: Vec<String>,
    /// `[validator_list_keys]` — hex-encoded publisher public keys.
    #[serde(default)]
    pub validator_list_keys: Vec<String>,
    /// `[amendments]` — names to vote *for*.
    #[serde(default)]
    pub amendments: Vec<String>,
    /// `[veto_amendments]` — names to vote *against*.
    #[serde(default)]
    pub veto_amendments: Vec<String>,
    /// `[features]` — names of features to enable.
    #[serde(default)]
    pub features: Vec<String>,
    /// `[cluster_nodes]` — `"<pubkey> [<name>]"` lines.
    #[serde(default)]
    pub cluster_nodes: Vec<String>,
    /// `[rpc_startup]` — array of JSON command strings.
    #[serde(default)]
    pub rpc_startup: Vec<String>,

    // ----- Scalar top-level keys (legacy single-value sections) -----
    pub debug_logfile: Option<PathBuf>,
    pub node_seed: Option<String>,
    pub validation_seed: Option<String>,
    pub validator_token: Option<String>,
    pub validator_key_revocation: Option<String>,
    pub validators_file: Option<PathBuf>,
    pub server_domain: Option<String>,

    // FFI (phase 2): polymorphic — needs a `{ kind, value }` cxx-shared
    // wrapper plus a `NetworkIdKind` enum (Main/Testnet/Devnet/Numeric).
    // Planned manual getters: `has_network_id()`, `network_id_kind()`,
    // `network_id_value()`.
    #[config_entry(skip)]
    pub network_id: Option<NetworkId>,
    pub network_quorum: Option<u32>,
    // FFI (phase 2): polymorphic — `NodeSizeKind` (Tiny/Small/Medium/Large/Huge/Numeric).
    // Planned: `has_node_size()`, `node_size_kind()`, `node_size_value()`.
    #[config_entry(skip)]
    pub node_size: Option<NodeSize>,
    // FFI (phase 2): polymorphic — `LedgerHistoryKind` (Full/None/Numeric).
    // Planned: `has_ledger_history()`, `ledger_history_kind()`, `ledger_history_value()`.
    #[config_entry(skip)]
    pub ledger_history: Option<LedgerHistory>,
    // FFI (phase 2): polymorphic — `FetchDepthKind` (Full/None/Numeric).
    // Planned: `has_fetch_depth()`, `fetch_depth_kind()`, `fetch_depth_value()`.
    #[config_entry(skip)]
    pub fetch_depth: Option<FetchDepth>,

    /// Drops. Overrides `voting.reference_fee` post-load when set.
    pub fee_default: Option<u64>,
    /// `[1, 1024]` when set; `0`/absent means auto.
    pub workers: Option<u32>,
    /// `[1, 1024]` when set; `0`/absent means auto (default 2).
    pub io_workers: Option<u32>,
    /// `[1, 1024]` when set; `0`/absent means auto (default 4).
    pub prefetch_workers: Option<u32>,
    /// Clamped to `[100, 1000]`. Default `250`.
    pub max_transactions: Option<u32>,
    /// Seconds. `[10, 600]` when set; otherwise derived from `node_size`.
    pub sweep_interval: Option<u32>,
    /// `"<n> <unit>"` where unit ∈ `minutes|hours|days|weeks`. Minimum 15 min.
    pub amendment_majority_time: Option<String>,

    /// `[ssl_verify]` (top-level scalar). Default `true`.
    pub ssl_verify: Option<bool>,
    pub ssl_verify_file: Option<PathBuf>,
    pub ssl_verify_dir: Option<PathBuf>,

    pub peer_private: Option<bool>,
    /// When set, `peers_in_max` / `peers_out_max` are ignored.
    pub peers_max: Option<u32>,
    /// Must be `<= 1000`. Must be set together with `peers_out_max`.
    pub peers_in_max: Option<u32>,
    /// Must be in `[10, 1000]`. Must be set together with `peers_in_max`.
    pub peers_out_max: Option<u32>,

    pub signing_support: Option<bool>,
    pub elb_support: Option<bool>,
    pub compression: Option<bool>,
    pub ledger_replay: Option<bool>,
    pub beta_rpc_api: Option<bool>,

    /// `[database_path]` — root directory for SQLite-backed bookkeeping DBs.
    pub database_path: Option<PathBuf>,

    pub path_search: Option<i32>,
    pub path_search_old: Option<i32>,
    pub path_search_fast: Option<i32>,
    pub path_search_max: Option<i32>,

    /// At most one line in the INI form; numeric `0` ⇒ auto.
    pub validator_list_threshold: Option<u32>,

    // FFI (phase 2): data-less enum — needs a cxx-shared `RelayMode` enum
    // (All/Trusted/DropUntrusted) plus an `OptionalRelayMode` wrapper.
    // Planned manual getter: `relay_proposals()` returning `OptionalRelayMode`.
    #[config_entry(skip)]
    pub relay_proposals: Option<RelayMode>,
    // FFI (phase 2): same `RelayMode` enum reused.
    // Planned: `relay_validations()` returning `OptionalRelayMode`.
    #[config_entry(skip)]
    pub relay_validations: Option<RelayMode>,

    // ----- Nested tables -----
    pub server: Option<Server>,
    pub grpc: Option<Grpc>,

    pub overlay: Option<Overlay>,
    pub reduce_relay: Option<ReduceRelay>,
    pub transaction_queue: Option<TransactionQueue>,
    pub hashrouter: Option<HashRouter>,

    // FFI (phase 2): internally-tagged enum (NuDB | RocksDB) with variant-
    // specific fields. Flat shape will be `{ kind, common: NodeDbCommon,
    // nudb_block_size, cache_mb, filter_bits }`.
    // Planned: `has_node_db()` + a `node_db()` returning a hand-written
    // `NodeDbView` opaque type whose getters expose the flat shape.
    #[config_entry(skip)]
    pub node_db: Option<NodeDb>,
    // FFI (phase 2): same shape as `node_db`. Planned: `has_import_db()` and
    // `import_db()` returning the same `NodeDbView`.
    #[config_entry(skip)]
    pub import_db: Option<NodeDb>,
    pub sqlite: Option<Sqlite>,
    pub sqdb: Option<Sqdb>,
    pub ledger_tx_tables: Option<LedgerTxTables>,

    pub insight: Option<Insight>,
    pub perf: Option<Perf>,
    pub voting: Option<Voting>,

    pub crawl: Option<Crawl>,
    pub vl: Option<Vl>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::database::{JournalMode, SafetyLevel, SqdbBackend, Synchronous, TempStore};
    use super::enums::{LedgerHistoryName, NetworkIdName, NodeSizeName};
    use super::server::{PortLimit, PortLimitName, Protocol};

    // Smoke tests for the TOML-shaped schema.
    //
    // These do not validate cross-section rules — they just confirm that
    // representative inputs deserialize into the expected variants.
    #[test]
    fn empty_doc_deserializes_to_default() {
        let cfg: Config = toml::from_str("").expect("empty TOML must parse");
        assert!(cfg.ips.is_empty());
        assert!(cfg.server.is_none());
        assert!(cfg.node_db.is_none());
    }

    #[test]
    fn polymorphic_scalars_accept_named_and_numeric_forms() {
        let cfg: Config = toml::from_str(
            r#"
            ledger_history = "full"
            fetch_depth    = 100000
            network_id     = "main"
            node_size      = "huge"
            relay_proposals   = "trusted"
            relay_validations = "drop_untrusted"
        "#,
        )
        .unwrap();

        assert_eq!(
            cfg.ledger_history,
            Some(LedgerHistory::Named(LedgerHistoryName::Full))
        );
        assert_eq!(cfg.fetch_depth, Some(FetchDepth::Numeric(100_000)));
        assert_eq!(cfg.network_id, Some(NetworkId::Named(NetworkIdName::Main)));
        assert_eq!(cfg.node_size, Some(NodeSize::Named(NodeSizeName::Huge)));
        assert_eq!(cfg.relay_proposals, Some(RelayMode::Trusted));
        assert_eq!(cfg.relay_validations, Some(RelayMode::DropUntrusted));
    }

    #[test]
    fn server_inherits_defaults_via_flatten() {
        let cfg: Config = toml::from_str(
            r#"
            [server]
            send_queue_limit = 500
            ssl_ciphers      = "HIGH:!aNULL"

            [server.ports.port_peer]
            ip       = "0.0.0.0"
            port     = 51235
            protocol = ["peer"]
            limit    = "unlimited"

            [server.ports.port_rpc]
            ip       = "127.0.0.1"
            port     = 5005
            protocol = ["http", "https"]
            admin    = ["127.0.0.1"]
            limit    = 200
        "#,
        )
        .unwrap();

        let srv = cfg.server.expect("server table present");
        assert_eq!(srv.defaults.send_queue_limit, Some(500));
        assert_eq!(srv.defaults.ssl_ciphers.as_deref(), Some("HIGH:!aNULL"));
        assert_eq!(srv.ports.len(), 2);

        let peer = &srv.ports["port_peer"];
        assert_eq!(peer.protocol.as_deref(), Some(&[Protocol::Peer][..]));
        assert_eq!(peer.limit, Some(PortLimit::Named(PortLimitName::Unlimited)));

        let rpc = &srv.ports["port_rpc"];
        assert_eq!(
            rpc.protocol.as_deref(),
            Some(&[Protocol::Http, Protocol::Https][..])
        );
        assert_eq!(rpc.limit, Some(PortLimit::Numeric(200)));
    }

    #[test]
    fn node_db_nudb_variant_accepts_block_size() {
        let cfg: Config = toml::from_str(
            r#"
            [node_db]
            type            = "NuDB"
            path            = "/var/lib/xrpld/nudb"
            online_delete   = 2000
            nudb_block_size = 4096
        "#,
        )
        .unwrap();

        match cfg.node_db.expect("node_db present") {
            NodeDb::NuDb(opts) => {
                assert_eq!(opts.common.online_delete, Some(2000));
                assert_eq!(opts.nudb_block_size, Some(4096));
            }
            NodeDb::RocksDb(_) => panic!("expected NuDB variant"),
        }
    }

    #[test]
    fn node_db_rocksdb_variant_accepts_cache_mb() {
        let cfg: Config = toml::from_str(
            r#"
            [node_db]
            type     = "RocksDB"
            path     = "/var/lib/xrpld/rocksdb"
            cache_mb = 512
        "#,
        )
        .unwrap();

        match cfg.node_db.expect("node_db present") {
            NodeDb::RocksDb(opts) => {
                assert_eq!(opts.cache_mb, Some(512));
            }
            NodeDb::NuDb(_) => panic!("expected RocksDB variant"),
        }
    }

    #[test]
    fn node_db_rejects_foreign_backend_keys() {
        // `nudb_block_size` is not valid on a RocksDB backend.
        let err = toml::from_str::<Config>(
            r#"
            [node_db]
            type            = "RocksDB"
            path            = "/var/lib/xrpld/rocksdb"
            nudb_block_size = 4096
        "#,
        )
        .err()
        .expect("RocksDB + nudb_block_size must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("nudb_block_size"), "unexpected error: {msg}");
    }

    #[test]
    fn sqlite_table_parses_named_enums() {
        let cfg: Config = toml::from_str(
            r#"
            [sqlite]
            safety_level = "low"
            journal_mode = "wal"
            synchronous  = "normal"
            temp_store   = "memory"
            page_size    = 4096
        "#,
        )
        .unwrap();
        let sqlite = cfg.sqlite.expect("sqlite present");
        assert_eq!(sqlite.safety_level, Some(SafetyLevel::Low));
        assert_eq!(sqlite.journal_mode, Some(JournalMode::Wal));
        assert_eq!(sqlite.synchronous, Some(Synchronous::Normal));
        assert_eq!(sqlite.temp_store, Some(TempStore::Memory));
    }

    #[test]
    fn unknown_top_level_key_is_a_hard_error() {
        let err = toml::from_str::<Config>("not_a_real_key = 1")
            .err()
            .expect("typo must be rejected");
        assert!(err.to_string().contains("not_a_real_key"));
    }

    #[test]
    fn list_style_top_level_arrays() {
        let cfg: Config = toml::from_str(
            r#"
            ips        = ["r.ripple.com 51235"]
            validators = ["n949f..."]
            features   = ["DeepFreeze", "PermissionedDEX"]
            rpc_startup = [
                '{ "command": "log_level", "severity": "warning" }',
            ]
        "#,
        )
        .unwrap();
        assert_eq!(cfg.ips, vec!["r.ripple.com 51235"]);
        assert_eq!(cfg.validators, vec!["n949f..."]);
        assert_eq!(cfg.features, vec!["DeepFreeze", "PermissionedDEX"]);
        assert_eq!(cfg.rpc_startup.len(), 1);
    }

    #[test]
    fn sqdb_backend_parses() {
        let cfg: Config = toml::from_str(
            r#"
            [sqdb]
            backend = "sqlite"
        "#,
        )
        .unwrap();
        assert_eq!(cfg.sqdb.unwrap().backend, Some(SqdbBackend::Sqlite));
    }
}

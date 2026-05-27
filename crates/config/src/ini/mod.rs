//! INI parser module.
//!
//! Stage 1 (`parser`) produces a `BasicConfig` from raw INI text.
//! Stage 2 (`mod.rs` + `accessors` + `builders`) constructs a typed `Config`
//! directly from `BasicConfig` via hand-written field-by-field queries.
//! No serde, no intermediate value type.
//!
//! Architecture:
//!   - `parser.rs`    — tokeniser / `BasicConfig` / `Section` types
//!   - `accessors.rs` — helpers for querying `BasicConfig`/`Section`
//!   - `builders.rs`  — per-nested-struct builders
//!   - `mod.rs`       — public entry point + top-level Config builder + validation + tests

pub mod accessors;
pub mod builders;
pub mod parser;

pub use parser::{BasicConfig, Section, parse_ini};

use std::collections::HashSet;
use std::path::PathBuf;

use crate::error::ParseError;
use crate::schema::Config;

use accessors::BasicConfigExt;
use builders::{
    build_crawl, build_grpc, build_hashrouter, build_insight, build_ledger_tx_tables,
    build_node_db, build_overlay, build_perf, build_reduce_relay, build_server, build_sqdb,
    build_sqlite, build_transaction_queue, build_vl, build_voting, parse_fetch_depth,
    parse_ledger_history, parse_network_id, parse_node_size, parse_relay_mode,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Construct `Config` directly from `BasicConfig` via hand-written
/// field-by-field queries.  No serde, no intermediate value type.
pub fn from_basic_config(bc: &BasicConfig) -> Result<Config, ParseError> {
    // Replaces serde's deny_unknown_fields at the section level.
    validate_top_level_sections(bc)?;

    // ---- Polymorphic / complex scalars ----
    let ledger_history = bc
        .scalar("ledger_history")
        .map(|s| parse_ledger_history(&s))
        .transpose()?;

    let fetch_depth = bc
        .scalar("fetch_depth")
        .map(|s| parse_fetch_depth(&s))
        .transpose()?;

    let network_id = bc
        .scalar("network_id")
        .map(|s| parse_network_id(&s))
        .transpose()?;

    let node_size = bc
        .scalar("node_size")
        .map(|s| parse_node_size(&s))
        .transpose()?;

    let relay_proposals = bc
        .scalar("relay_proposals")
        .map(|s| parse_relay_mode(&s, "relay_proposals"))
        .transpose()?;

    let relay_validations = bc
        .scalar("relay_validations")
        .map(|s| parse_relay_mode(&s, "relay_validations"))
        .transpose()?;

    // ---- Simple scalar sections ----
    // These are single-value sections (value-lines, not kv pairs).
    let debug_logfile: Option<PathBuf> = bc.scalar("debug_logfile").map(PathBuf::from);
    let node_seed: Option<String> = bc.scalar("node_seed");
    let validation_seed: Option<String> = bc.scalar("validation_seed");

    // 7. Multi-line validator_token / validator_key_revocation: concat all lines.
    let validator_token: Option<String> = {
        let sec = bc.get("validator_token");
        match sec {
            None => None,
            Some(s) if s.values.is_empty() => None,
            Some(s) => Some(s.values.concat()),
        }
    };
    let validator_key_revocation: Option<String> = {
        let sec = bc.get("validator_key_revocation");
        match sec {
            None => None,
            Some(s) if s.values.is_empty() => None,
            Some(s) => Some(s.values.concat()),
        }
    };

    let validators_file: Option<PathBuf> = bc.scalar("validators_file").map(PathBuf::from);
    let server_domain: Option<String> = bc.scalar("server_domain");

    let network_quorum: Option<u32> = bc.scalar_parse("network_quorum")?;
    let fee_default: Option<u64> = bc.scalar_parse("fee_default")?;
    let workers: Option<u32> = bc.scalar_parse("workers")?;
    let io_workers: Option<u32> = bc.scalar_parse("io_workers")?;
    let prefetch_workers: Option<u32> = bc.scalar_parse("prefetch_workers")?;
    let max_transactions: Option<u32> = bc.scalar_parse("max_transactions")?;
    let sweep_interval: Option<u32> = bc.scalar_parse("sweep_interval")?;
    let amendment_majority_time: Option<String> = bc.scalar("amendment_majority_time");

    let ssl_verify: Option<bool> = bc.scalar_bool("ssl_verify")?;
    let ssl_verify_file: Option<PathBuf> = bc.scalar("ssl_verify_file").map(PathBuf::from);
    let ssl_verify_dir: Option<PathBuf> = bc.scalar("ssl_verify_dir").map(PathBuf::from);

    let peer_private: Option<bool> = bc.scalar_bool("peer_private")?;
    let peers_max: Option<u32> = bc.scalar_parse("peers_max")?;
    let peers_in_max: Option<u32> = bc.scalar_parse("peers_in_max")?;
    let peers_out_max: Option<u32> = bc.scalar_parse("peers_out_max")?;

    let signing_support: Option<bool> = bc.scalar_bool("signing_support")?;
    let elb_support: Option<bool> = bc.scalar_bool("elb_support")?;
    let compression: Option<bool> = bc.scalar_bool("compression")?;
    let ledger_replay: Option<bool> = bc.scalar_bool("ledger_replay")?;
    let beta_rpc_api: Option<bool> = bc.scalar_bool("beta_rpc_api")?;

    let database_path: Option<PathBuf> = bc.scalar("database_path").map(PathBuf::from);

    let path_search: Option<i32> = bc.scalar_parse("path_search")?;
    let path_search_old: Option<i32> = bc.scalar_parse("path_search_old")?;
    let path_search_fast: Option<i32> = bc.scalar_parse("path_search_fast")?;
    let path_search_max: Option<i32> = bc.scalar_parse("path_search_max")?;

    let validator_list_threshold: Option<u32> = bc.scalar_parse("validator_list_threshold")?;

    // ---- Build all struct fields — struct literal enforces exhaustiveness ----
    Ok(Config {
        // List-style sections
        ips:                   bc.values_of("ips"),
        ips_fixed:             bc.values_of("ips_fixed"),
        validators:            bc.values_of("validators"),
        validator_keys:        bc.values_of("validator_keys"),
        validator_list_sites:  bc.values_of("validator_list_sites"),
        validator_list_keys:   bc.values_of("validator_list_keys"),
        amendments:            bc.values_of("amendments"),
        veto_amendments:       bc.values_of("veto_amendments"),
        features:              bc.values_of("features"),
        cluster_nodes:         bc.values_of("cluster_nodes"),
        rpc_startup:           bc.values_of("rpc_startup"),

        // Simple scalar fields
        debug_logfile,
        node_seed,
        validation_seed,
        validator_token,
        validator_key_revocation,
        validators_file,
        server_domain,

        // Polymorphic scalars
        network_id,
        network_quorum,
        node_size,
        ledger_history,
        fetch_depth,

        fee_default,
        workers,
        io_workers,
        prefetch_workers,
        max_transactions,
        sweep_interval,
        amendment_majority_time,

        ssl_verify,
        ssl_verify_file,
        ssl_verify_dir,

        peer_private,
        peers_max,
        peers_in_max,
        peers_out_max,

        signing_support,
        elb_support,
        compression,
        ledger_replay,
        beta_rpc_api,

        database_path,

        path_search,
        path_search_old,
        path_search_fast,
        path_search_max,

        validator_list_threshold,

        relay_proposals,
        relay_validations,

        // Nested tables
        server:           build_server(bc)?,
        grpc:             build_grpc(bc)?,   // 1. port_grpc → grpc rename
        overlay:          build_overlay(bc)?,
        reduce_relay:     build_reduce_relay(bc)?,
        transaction_queue: build_transaction_queue(bc)?,
        hashrouter:       build_hashrouter(bc)?,
        node_db:          build_node_db(bc, "node_db")?,
        import_db:        build_node_db(bc, "import_db")?,
        sqlite:           build_sqlite(bc)?,
        sqdb:             build_sqdb(bc)?,
        ledger_tx_tables: build_ledger_tx_tables(bc)?,
        insight:          build_insight(bc)?,
        perf:             build_perf(bc)?,
        voting:           build_voting(bc)?,
        crawl:            build_crawl(bc)?,  // 5. [crawl] value-line lift
        vl:               build_vl(bc)?,     // 6. [vl] enable alias
    })
}

// ---------------------------------------------------------------------------
// validate_top_level_sections
//
// Replaces serde's deny_unknown_fields at the section level.  This is the
// complete, frozen INI section catalog — INI is deprecated and never gets
// new entries.
// ---------------------------------------------------------------------------

fn validate_top_level_sections(bc: &BasicConfig) -> Result<(), ParseError> {
    /// Every top-level section name accepted by the schema (sorted).
    ///
    /// This list is intentionally frozen — INI is being deprecated; new
    /// fields go to TOML only.  The 62 entries below are the complete
    /// legacy section catalog.
    const KNOWN: &[&str] = &[
        "amendment_majority_time",
        "amendments",
        "beta_rpc_api",
        "cluster_nodes",
        "compression",
        "crawl",
        "database_path",
        "debug_logfile",
        "elb_support",
        "features",
        "fee_default",
        "fetch_depth",
        "hashrouter",
        "import_db",
        "insight",
        "io_workers",
        "ips",
        "ips_fixed",
        "ledger_history",
        "ledger_replay",
        "ledger_tx_tables",
        "max_transactions",
        "network_id",
        "network_quorum",
        "node_db",
        "node_seed",
        "node_size",
        "overlay",
        "path_search",
        "path_search_fast",
        "path_search_max",
        "path_search_old",
        "peer_private",
        "peers_in_max",
        "peers_max",
        "peers_out_max",
        "perf",
        "port_grpc",
        "prefetch_workers",
        "reduce_relay",
        "relay_proposals",
        "relay_validations",
        "rpc_startup",
        "server",
        "server_domain",
        "signing_support",
        "sqdb",
        "sqlite",
        "ssl_verify",
        "ssl_verify_dir",
        "ssl_verify_file",
        "sweep_interval",
        "transaction_queue",
        "validation_seed",
        "validator_key_revocation",
        "validator_keys",
        "validator_list_keys",
        "validator_list_sites",
        "validator_list_threshold",
        "validator_token",
        "validators",
        "validators_file",
        "veto_amendments",
        "vl",
        "voting",
        "workers",
    ];

    // 8. Reserved sections are silently ignored.
    const RESERVED: &[&str] = &["sntp_servers", "websocket_ping_frequency", "relational_db"];

    // Collect port section names from [server].values so we can allow them.
    let port_names: HashSet<&str> = bc
        .get("server")
        .map(|s| s.values.iter().map(String::as_str).collect())
        .unwrap_or_default();

    for name in bc.keys() {
        let name = name.as_str();
        // 8. Default empty-name section and reserved sections are silently dropped.
        if name.is_empty() || RESERVED.contains(&name) {
            continue;
        }
        // Dynamic port sections (listed in [server].values) are valid.
        if port_names.contains(name) {
            continue;
        }
        if !KNOWN.contains(&name) {
            return Err(ParseError::Ini(format!(
                "unknown INI section [{name}]"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::from_basic_config;
    use crate::ini::parser::parse_ini;
    use crate::schema::database::NodeDb;
    use crate::schema::enums::{LedgerHistory, LedgerHistoryName};

    fn parse(ini: &str) -> crate::schema::Config {
        let bc = parse_ini(ini);
        from_basic_config(&bc).expect("from_basic_config failed")
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 1: port_grpc → grpc rename
    // -----------------------------------------------------------------------

    #[test]
    fn port_grpc_lifts_to_grpc() {
        let ini = "[port_grpc]\nip = 127.0.0.1\nport = 50051\nsecure_gateway = 127.0.0.1";
        let cfg = parse(ini);
        let grpc = cfg.grpc.unwrap();
        assert_eq!(grpc.ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(grpc.port, Some(50051));
        assert_eq!(
            grpc.secure_gateway.as_deref(),
            Some(&["127.0.0.1".to_string()][..])
        );
    }

    #[test]
    fn port_grpc_csv_secure_gateway() {
        let ini = "[port_grpc]\nip = 127.0.0.1\nport = 50051\nsecure_gateway = 127.0.0.1,::1";
        let cfg = parse(ini);
        let grpc = cfg.grpc.unwrap();
        assert_eq!(grpc.secure_gateway.as_ref().map(|v| v.len()), Some(2));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 2: [server] + per-port lifting
    // -----------------------------------------------------------------------

    #[test]
    fn server_port_lifting() {
        let ini = r"
[server]
port_peer
port_rpc

[port_peer]
ip = 0.0.0.0
port = 51235
protocol = peer

[port_rpc]
ip = 127.0.0.1
port = 5005
protocol = http,https
admin = 127.0.0.1
";
        let cfg = parse(ini);
        let server = cfg.server.unwrap();
        let peer = server.ports.get("port_peer").unwrap();
        assert_eq!(peer.ip.as_deref(), Some("0.0.0.0"));
        assert_eq!(peer.port, Some(51235));
        assert_eq!(peer.protocol.as_ref().map(|p| p.len()), Some(1));

        let rpc = server.ports.get("port_rpc").unwrap();
        assert_eq!(rpc.protocol.as_ref().map(|p| p.len()), Some(2));
        assert_eq!(rpc.admin.as_ref().map(|a| a.len()), Some(1));
    }

    #[test]
    fn server_port_grpc_excluded_from_ports_map() {
        let ini = "[server]\nport_grpc\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=http\n\n[port_grpc]\nip=127.0.0.1\nport=50051";
        let cfg = parse(ini);
        let server = cfg.server.unwrap();
        // port_grpc should NOT appear in ports map
        assert!(!server.ports.contains_key("port_grpc"));
        // port_rpc should be present
        assert!(server.ports.contains_key("port_rpc"));
        // grpc should be built from port_grpc
        assert!(cfg.grpc.is_some());
    }

    #[test]
    fn missing_port_section_is_error() {
        let bc = parse_ini("[server]\nport_missing");
        let err = from_basic_config(&bc).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("port_missing"),
            "unexpected error: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 3: node_db type canonicalization (case-insensitive)
    // -----------------------------------------------------------------------

    #[test]
    fn node_db_nudb_lowercase() {
        let cfg = parse("[node_db]\ntype = nudb\npath = /db");
        match cfg.node_db.unwrap() {
            NodeDb::NuDb(opts) => assert_eq!(opts.common.path.to_str(), Some("/db")),
            NodeDb::RocksDb(_) => panic!("expected NuDB"),
        }
    }

    #[test]
    fn node_db_nudb_uppercase() {
        let cfg = parse("[node_db]\ntype = NUDB\npath = /db");
        match cfg.node_db.unwrap() {
            NodeDb::NuDb(_) => {}
            NodeDb::RocksDb(_) => panic!("expected NuDB"),
        }
    }

    #[test]
    fn node_db_rocksdb_case_insensitive() {
        let cfg = parse("[node_db]\ntype = RocksDB\npath = /db\ncache_mb = 512");
        match cfg.node_db.unwrap() {
            NodeDb::RocksDb(opts) => assert_eq!(opts.cache_mb, Some(512)),
            NodeDb::NuDb(_) => panic!("expected RocksDB"),
        }
    }

    #[test]
    fn node_db_unknown_type_is_error() {
        let bc = parse_ini("[node_db]\ntype = LevelDB\npath = /db");
        let err = from_basic_config(&bc).unwrap_err();
        assert!(
            err.to_string().contains("leveldb") || err.to_string().contains("LevelDB"),
            "{err}"
        );
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 4: comma-split protocol, admin, secure_gateway
    // -----------------------------------------------------------------------

    #[test]
    fn comma_split_protocol() {
        let ini = "[server]\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=http,https";
        let cfg = parse(ini);
        let port = cfg.server.unwrap().ports.get("port_rpc").unwrap().clone();
        assert_eq!(port.protocol.as_ref().map(|p| p.len()), Some(2));
    }

    #[test]
    fn comma_split_protocol_mixed_case() {
        let ini =
            "[server]\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=HTTP,HTTPS";
        let cfg = parse(ini);
        let port = cfg.server.unwrap().ports.get("port_rpc").unwrap().clone();
        assert_eq!(port.protocol.as_ref().map(|p| p.len()), Some(2));
    }

    #[test]
    fn comma_split_admin() {
        let ini =
            "[server]\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=http\nadmin=127.0.0.1,::1";
        let cfg = parse(ini);
        let port = cfg.server.unwrap().ports.get("port_rpc").unwrap().clone();
        assert_eq!(port.admin.as_ref().map(|a| a.len()), Some(2));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 5: [crawl] 0|1 value-line lift
    // -----------------------------------------------------------------------

    #[test]
    fn crawl_value_line_lift_enabled_true() {
        let cfg = parse("[crawl]\n1");
        assert_eq!(cfg.crawl.unwrap().enabled, Some(true));
    }

    #[test]
    fn crawl_value_line_lift_enabled_false() {
        let cfg = parse("[crawl]\n0");
        assert_eq!(cfg.crawl.unwrap().enabled, Some(false));
    }

    #[test]
    fn crawl_kv_pairs_parsed() {
        let cfg = parse("[crawl]\noverlay = 1\ncounts = 0");
        let crawl = cfg.crawl.unwrap();
        assert_eq!(crawl.overlay, Some(true));
        assert_eq!(crawl.counts, Some(false));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 6: [vl] enable → enabled alias
    // -----------------------------------------------------------------------

    #[test]
    fn vl_enable_alias() {
        let cfg = parse("[vl]\nenable = 1");
        assert_eq!(cfg.vl.unwrap().enabled, Some(true));
    }

    #[test]
    fn vl_enabled_direct() {
        let cfg = parse("[vl]\nenabled = true");
        assert_eq!(cfg.vl.unwrap().enabled, Some(true));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 7: multi-line validator_token / validator_key_revocation
    // -----------------------------------------------------------------------

    #[test]
    fn validator_token_multiline_concat() {
        let ini = "[validator_token]\nlineA\nlineB";
        let cfg = parse(ini);
        assert_eq!(cfg.validator_token.as_deref(), Some("lineAlineB"));
    }

    #[test]
    fn validator_key_revocation_multiline() {
        let ini = "[validator_key_revocation]\npartA\npartB";
        let cfg = parse(ini);
        assert_eq!(cfg.validator_key_revocation.as_deref(), Some("partApartB"));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 8: reserved sections silently dropped
    // -----------------------------------------------------------------------

    #[test]
    fn reserved_section_sntp_dropped() {
        let cfg = parse("[sntp_servers]\nfoo.bar");
        assert!(cfg.network_quorum.is_none());
    }

    #[test]
    fn reserved_section_websocket_ping_frequency_dropped() {
        let cfg = parse("[websocket_ping_frequency]\n60");
        assert!(cfg.network_quorum.is_none());
    }

    #[test]
    fn reserved_section_relational_db_dropped() {
        let cfg = parse("[relational_db]\nval");
        assert!(cfg.network_quorum.is_none());
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 9: polymorphic scalars
    // -----------------------------------------------------------------------

    #[test]
    fn ledger_history_named_full() {
        let cfg = parse("[ledger_history]\nfull");
        assert_eq!(
            cfg.ledger_history,
            Some(LedgerHistory::Named(LedgerHistoryName::Full))
        );
    }

    #[test]
    fn ledger_history_named_none() {
        use crate::schema::enums::LedgerHistoryName;
        let cfg = parse("[ledger_history]\nnone");
        assert_eq!(
            cfg.ledger_history,
            Some(LedgerHistory::Named(LedgerHistoryName::None))
        );
    }

    #[test]
    fn ledger_history_numeric() {
        let cfg = parse("[ledger_history]\n256");
        assert_eq!(cfg.ledger_history, Some(LedgerHistory::Numeric(256)));
    }

    #[test]
    fn fetch_depth_named_full() {
        use crate::schema::enums::{FetchDepth, FetchDepthName};
        let cfg = parse("[fetch_depth]\nfull");
        assert_eq!(cfg.fetch_depth, Some(FetchDepth::Named(FetchDepthName::Full)));
    }

    #[test]
    fn fetch_depth_numeric() {
        use crate::schema::enums::FetchDepth;
        let cfg = parse("[fetch_depth]\n1000000000");
        assert_eq!(cfg.fetch_depth, Some(FetchDepth::Numeric(1_000_000_000)));
    }

    #[test]
    fn node_size_named_huge() {
        use crate::schema::enums::{NodeSize, NodeSizeName};
        let cfg = parse("[node_size]\nhuge");
        assert_eq!(cfg.node_size, Some(NodeSize::Named(NodeSizeName::Huge)));
    }

    #[test]
    fn node_size_numeric() {
        use crate::schema::enums::NodeSize;
        let cfg = parse("[node_size]\n3");
        assert_eq!(cfg.node_size, Some(NodeSize::Numeric(3)));
    }

    #[test]
    fn network_id_named_main() {
        use crate::schema::enums::{NetworkId, NetworkIdName};
        let cfg = parse("[network_id]\nmain");
        assert_eq!(cfg.network_id, Some(NetworkId::Named(NetworkIdName::Main)));
    }

    #[test]
    fn network_id_numeric() {
        use crate::schema::enums::NetworkId;
        let cfg = parse("[network_id]\n1234");
        assert_eq!(cfg.network_id, Some(NetworkId::Numeric(1234)));
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 10: C++ bool dialect
    // -----------------------------------------------------------------------

    #[test]
    fn bool_dialects_true() {
        for val in &["yes", "on", "1", "true", "Yes", "ON", "TRUE"] {
            let ini = format!("[ssl_verify]\n{val}");
            let cfg = parse(&ini);
            assert_eq!(cfg.ssl_verify, Some(true), "failed for '{val}'");
        }
    }

    #[test]
    fn bool_dialects_false() {
        for val in &["no", "off", "0", "false", "No", "OFF", "FALSE"] {
            let ini = format!("[ssl_verify]\n{val}");
            let cfg = parse(&ini);
            assert_eq!(cfg.ssl_verify, Some(false), "failed for '{val}'");
        }
    }

    // -----------------------------------------------------------------------
    // Bespoke quirk 11: enum-name lowercasing
    // -----------------------------------------------------------------------

    #[test]
    fn sqlite_safety_level_uppercase() {
        use crate::schema::database::SafetyLevel;
        let cfg = parse("[sqlite]\nsafety_level = LOW");
        assert_eq!(cfg.sqlite.unwrap().safety_level, Some(SafetyLevel::Low));
    }

    #[test]
    fn sqlite_journal_mode_uppercase() {
        use crate::schema::database::JournalMode;
        let cfg = parse("[sqlite]\njournal_mode = WAL");
        assert_eq!(cfg.sqlite.unwrap().journal_mode, Some(JournalMode::Wal));
    }

    #[test]
    fn relay_proposals_trusted_mixed_case() {
        use crate::schema::enums::RelayMode;
        let cfg = parse("[relay_proposals]\nTrusted");
        assert_eq!(cfg.relay_proposals, Some(RelayMode::Trusted));
    }

    #[test]
    fn relay_validations_drop_untrusted() {
        use crate::schema::enums::RelayMode;
        let cfg = parse("[relay_validations]\ndrop_untrusted");
        assert_eq!(cfg.relay_validations, Some(RelayMode::DropUntrusted));
    }

    #[test]
    fn sqdb_backend_lowercase() {
        use crate::schema::database::SqdbBackend;
        let cfg = parse("[sqdb]\nbackend = sqlite");
        assert_eq!(cfg.sqdb.unwrap().backend, Some(SqdbBackend::Sqlite));
    }

    #[test]
    fn insight_server_statsd() {
        let cfg = parse("[insight]\nserver = statsd");
        cfg.insight.unwrap().server.unwrap();
    }

    // -----------------------------------------------------------------------
    // Unknown top-level section → error
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_top_level_section_is_error() {
        let bc = parse_ini("[totally_unknown_section_xyz]\nval");
        let err = from_basic_config(&bc).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("totally_unknown_section_xyz"),
            "unexpected error: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage
    // -----------------------------------------------------------------------

    #[test]
    fn value_list_ips() {
        let cfg = parse("[ips]\nh1\nh2");
        assert_eq!(cfg.ips, vec!["h1", "h2"]);
    }

    #[test]
    fn value_list_validators() {
        let cfg = parse("[validators]\nnABC\nnDEF");
        assert_eq!(cfg.validators, vec!["nABC", "nDEF"]);
    }

    #[test]
    fn scalar_network_quorum_integer() {
        let cfg = parse("[network_quorum]\n3");
        assert_eq!(cfg.network_quorum, Some(3));
    }

    #[test]
    fn kv_table_overlay() {
        let cfg = parse("[overlay]\nmax_unknown_time = 600");
        assert_eq!(cfg.overlay.unwrap().max_unknown_time, Some(600));
    }

    #[test]
    fn import_db_parsed() {
        let cfg = parse("[import_db]\ntype = NuDB\npath = /import");
        match cfg.import_db.unwrap() {
            NodeDb::NuDb(opts) => assert_eq!(opts.common.path.to_str(), Some("/import")),
            _ => panic!("expected NuDB"),
        }
    }

    #[test]
    fn port_limit_unlimited() {
        use crate::schema::server::{PortLimit, PortLimitName};
        let ini = "[server]\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=http\nlimit=unlimited";
        let cfg = parse(ini);
        let port = cfg.server.unwrap().ports.get("port_rpc").unwrap().clone();
        assert_eq!(port.limit, Some(PortLimit::Named(PortLimitName::Unlimited)));
    }

    #[test]
    fn port_limit_numeric() {
        use crate::schema::server::PortLimit;
        let ini = "[server]\nport_rpc\n\n[port_rpc]\nip=127.0.0.1\nport=5005\nprotocol=http\nlimit=200";
        let cfg = parse(ini);
        let port = cfg.server.unwrap().ports.get("port_rpc").unwrap().clone();
        assert_eq!(port.limit, Some(PortLimit::Numeric(200)));
    }
}

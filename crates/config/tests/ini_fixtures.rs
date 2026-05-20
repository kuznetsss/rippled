//! Integration tests: load every `fixtures/ini/*.cfg` file, assert it parses,
//! spot-check key getters, and verify that bootstrap succeeds.

mod common;

use config::{
    Config, CrawlConfig, HostKind, LedgerHistory, NodeDbKind, SqliteMode, SqliteSafety,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load a fixture file, parse it as INI, run bootstrap (standalone), return the Config.
fn load_ini(filename: &str) -> Config {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ini")
        .join(filename);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    common::parse_ini_bootstrap(&text)
}

// ---------------------------------------------------------------------------
// minimal.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_minimal_parses_with_defaults() {
    let cfg = load_ini("minimal.cfg");
    // Defaults from Parsed::default()
    assert_eq!(cfg.network_id(), 0);
    assert_eq!(cfg.network_quorum(), 1);
    assert!(!cfg.peer_private());
    assert_eq!(cfg.max_transactions(), 250);
    assert_eq!(cfg.ledger_history(), LedgerHistory::None_); // standalone forces None_
    assert!(cfg.trusted_validators().is_empty());
    assert!(cfg.ips().is_empty());
    assert!(cfg.server().port_names.is_empty());
}

// ---------------------------------------------------------------------------
// overlay.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_overlay_explicit_values() {
    let cfg = load_ini("overlay.cfg");
    assert_eq!(cfg.overlay().max_unknown_time, 900);
    assert_eq!(cfg.overlay().max_diverged_time, 120);
    assert_eq!(cfg.overlay().ip_limit, Some(3));
}

// ---------------------------------------------------------------------------
// node_db.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_node_db_nudb_settings() {
    let cfg = load_ini("node_db.cfg");
    let db = cfg.node_db();
    assert_eq!(db.kind, NodeDbKind::NuDb);
    assert_eq!(db.online_delete, Some(512));
    assert!(!db.advisory_delete);
    assert_eq!(db.earliest_seq, 1);
    assert_eq!(db.path.to_string_lossy(), "/tmp/test_nudb");
}

// ---------------------------------------------------------------------------
// sqlite_safety.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_sqlite_safety_level() {
    let cfg = load_ini("sqlite_safety.cfg");
    assert!(
        matches!(cfg.sqlite().mode, SqliteMode::Safety { level: SqliteSafety::High }),
        "expected Safety{{High}}, got {:?}",
        cfg.sqlite().mode
    );
    assert_eq!(cfg.sqlite().journal_size_limit, 2_000_000);
}

// ---------------------------------------------------------------------------
// sqlite_tuning.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_sqlite_tuning_mode() {
    let cfg = load_ini("sqlite_tuning.cfg");
    assert!(
        matches!(cfg.sqlite().mode, SqliteMode::Tuning { .. }),
        "expected Tuning, got {:?}",
        cfg.sqlite().mode
    );
    if let SqliteMode::Tuning { page_size, .. } = cfg.sqlite().mode {
        assert_eq!(page_size, 4096);
    }
}

// ---------------------------------------------------------------------------
// server_and_ports.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_server_and_ports() {
    let cfg = load_ini("server_and_ports.cfg");
    let server = cfg.server();
    assert_eq!(server.port_names.len(), 3);
    assert!(server.port_names.contains(&"port_rpc_admin_local".to_owned()));
    assert!(server.port_names.contains(&"port_peer".to_owned()));
    assert!(server.port_names.contains(&"port_ws_admin_local".to_owned()));

    let rpc = cfg.port("port_rpc_admin_local").expect("rpc port missing");
    assert_eq!(rpc.port, 5005);

    let peer = cfg.port("port_peer").expect("peer port missing");
    assert_eq!(peer.port, 51235);

    let ws = cfg.port("port_ws_admin_local").expect("ws port missing");
    assert_eq!(ws.port, 6006);

    // server-level send_queue_limit=500 is inherited by all ports via apply_port_defaults
    // (only when the port has the default value of 100, which it does here)
    assert_eq!(cfg.server().defaults.send_queue_limit, 500);
}

// ---------------------------------------------------------------------------
// validators.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_validators_bare_lines() {
    let cfg = load_ini("validators.cfg");
    let vs = cfg.trusted_validators();
    // [validators]: 2 entries, [validator_keys]: 1 entry → 3 total
    assert_eq!(vs.len(), 3, "expected 3 validators, got {}", vs.len());

    let alpha = vs.iter().find(|v| v.key == "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu");
    assert!(alpha.is_some(), "validator key Alpha not found");
    assert_eq!(alpha.unwrap().label.as_deref(), Some("Validator Alpha"));

    let unlabelled = vs.iter().find(|v| v.key == "nHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK");
    assert!(unlabelled.is_some(), "unlabelled key not found");
    assert!(unlabelled.unwrap().label.is_none());
}

// ---------------------------------------------------------------------------
// ips.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_ips_and_ips_fixed() {
    let cfg = load_ini("ips.cfg");
    assert_eq!(cfg.ips().len(), 3);
    assert_eq!(cfg.ips_fixed().len(), 1);

    // Spot-check that the host names parsed correctly.
    let r_ripple = cfg.ips().iter().find(|hp| {
        matches!(&hp.host, HostKind::Hostname(h) if h == "r.ripple.com")
    });
    assert!(r_ripple.is_some(), "r.ripple.com not found in ips");
    assert_eq!(r_ripple.unwrap().port, Some(51235));

    let s1_ripple = cfg.ips().iter().find(|hp| {
        matches!(&hp.host, HostKind::Hostname(h) if h == "s1.ripple.com")
    });
    assert!(s1_ripple.is_some(), "s1.ripple.com not found in ips");
}

// ---------------------------------------------------------------------------
// crawl_legacy.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_crawl_legacy_bool_form() {
    let cfg = load_ini("crawl_legacy.cfg");
    assert!(
        matches!(cfg.crawl(), CrawlConfig::LegacyBool(true)),
        "expected LegacyBool(true), got {:?}",
        cfg.crawl()
    );
}

// ---------------------------------------------------------------------------
// crawl_detailed.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_crawl_detailed_form() {
    let cfg = load_ini("crawl_detailed.cfg");
    assert!(
        matches!(
            cfg.crawl(),
            CrawlConfig::Detailed { overlay: true, server: true, counts: false, unl: true }
        ),
        "unexpected crawl: {:?}",
        cfg.crawl()
    );
}

// ---------------------------------------------------------------------------
// lenient_clamp.cfg
// ---------------------------------------------------------------------------

#[test]
fn ini_lenient_clamp_max_transactions() {
    // INI fixture has max_transactions=50 (below minimum 100).
    // Lenient INI must clamp to 100, NOT reject or leave at 50.
    let cfg = load_ini("lenient_clamp.cfg");
    assert_eq!(
        cfg.max_transactions(),
        100,
        "INI should clamp max_transactions=50 to 100"
    );
}

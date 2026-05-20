//! Integration tests: load every `fixtures/toml/*.toml` file, assert it parses,
//! spot-check key getters, and verify that bootstrap succeeds.

mod common;

use config::{
    Config, CrawlConfig, HostKind, LedgerHistory, NodeDbKind, SqliteMode, SqliteSafety,
};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn load_toml(filename: &str) -> Config {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/toml")
        .join(filename);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    common::parse_toml_bootstrap(&text)
}

// ---------------------------------------------------------------------------
// minimal.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_minimal_parses_with_defaults() {
    let cfg = load_toml("minimal.toml");
    assert_eq!(cfg.network_id(), 0);
    // TOML default for network_quorum is 0 (u64 default); INI default is 1.
    // This is a known format asymmetry — TOML uses numeric defaults from serde.
    assert_eq!(cfg.network_quorum(), 0);
    assert!(!cfg.peer_private());
    assert_eq!(cfg.max_transactions(), 250);
    assert_eq!(cfg.ledger_history(), LedgerHistory::None_); // standalone forces None_
    assert!(cfg.trusted_validators().is_empty());
    assert!(cfg.ips().is_empty());
}

// ---------------------------------------------------------------------------
// overlay.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_overlay_explicit_values() {
    let cfg = load_toml("overlay.toml");
    assert_eq!(cfg.overlay().max_unknown_time, 900);
    assert_eq!(cfg.overlay().max_diverged_time, 120);
    assert_eq!(cfg.overlay().ip_limit, Some(3));
}

// ---------------------------------------------------------------------------
// node_db.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_node_db_nudb_settings() {
    let cfg = load_toml("node_db.toml");
    let db = cfg.node_db();
    assert_eq!(db.kind, NodeDbKind::NuDb);
    assert_eq!(db.online_delete, Some(512));
    assert!(!db.advisory_delete);
    assert_eq!(db.earliest_seq, 1);
    assert_eq!(db.path.to_string_lossy(), "/tmp/test_nudb");
}

// ---------------------------------------------------------------------------
// sqlite.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_sqlite_safety_level() {
    let cfg = load_toml("sqlite.toml");
    assert!(
        matches!(cfg.sqlite().mode, SqliteMode::Safety { level: SqliteSafety::High }),
        "expected Safety{{High}}, got {:?}",
        cfg.sqlite().mode
    );
    assert_eq!(cfg.sqlite().journal_size_limit, 2_000_000);
}

// ---------------------------------------------------------------------------
// server_and_ports.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_server_and_ports() {
    let cfg = load_toml("server_and_ports.toml");
    let server = cfg.server();
    assert_eq!(server.port_names.len(), 3);
    assert!(server.port_names.contains(&"rpc_admin".to_owned()));
    assert!(server.port_names.contains(&"peer".to_owned()));
    assert!(server.port_names.contains(&"ws_admin".to_owned()));

    let rpc = cfg.port("rpc_admin").expect("rpc_admin port missing");
    assert_eq!(rpc.port, 5005);

    let peer_port = cfg.port("peer").expect("peer port missing");
    assert_eq!(peer_port.port, 51235);

    let ws = cfg.port("ws_admin").expect("ws_admin port missing");
    assert_eq!(ws.port, 6006);
    assert_eq!(ws.effective.send_queue_limit, 500);
}

// ---------------------------------------------------------------------------
// validators.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_validators_inline_table_array() {
    let cfg = load_toml("validators.toml");
    let vs = cfg.trusted_validators();
    assert_eq!(vs.len(), 3, "expected 3 validators, got {}", vs.len());

    let alpha = vs.iter().find(|v| v.key == "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu");
    assert!(alpha.is_some(), "validator Alpha not found");
    assert_eq!(alpha.unwrap().label.as_deref(), Some("Validator Alpha"));

    let unlabelled = vs.iter().find(|v| v.key == "nHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK");
    assert!(unlabelled.is_some(), "unlabelled validator not found");
    assert!(unlabelled.unwrap().label.is_none());
}

// ---------------------------------------------------------------------------
// ips.toml
// ---------------------------------------------------------------------------

#[test]
fn toml_ips_and_ips_fixed() {
    let cfg = load_toml("ips.toml");
    assert_eq!(cfg.ips().len(), 3);
    assert_eq!(cfg.ips_fixed().len(), 1);

    let r_ripple = cfg.ips().iter().find(|hp| {
        matches!(&hp.host, HostKind::Hostname(h) if h == "r.ripple.com")
    });
    assert!(r_ripple.is_some(), "r.ripple.com not found in ips");
    assert_eq!(r_ripple.unwrap().port, Some(51235));
}

// ---------------------------------------------------------------------------
// TOML-specific: crawl section (detailed form only; no legacy bool in TOML)
// ---------------------------------------------------------------------------

#[test]
fn toml_crawl_detailed_form_inline() {
    let text = r#"
[crawl]
overlay = true
server = true
counts = false
unl = false
"#;
    let cfg = common::parse_toml_bootstrap(text);
    assert!(
        matches!(
            cfg.crawl(),
            CrawlConfig::Detailed { overlay: true, server: true, counts: false, unl: false }
        ),
        "unexpected crawl: {:?}",
        cfg.crawl()
    );
}

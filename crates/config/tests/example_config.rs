//! Integration tests for `cfg/xrpld-example.cfg` — the canonical rippled config.
//!
//! These tests parse the real example config shipped with xrpld and verify that
//! it round-trips correctly through the INI parser.  This is the regression gate
//! required by design §13.2.

use std::path::{Path, PathBuf};
use config::{Config, NodeDbKind};

/// Path to the repo root, derived from `CARGO_MANIFEST_DIR`.
fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/config`
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()   // crates/
        .expect("crates/")
        .parent()   // repo root
        .expect("repo root")
        .to_owned()
}

fn example_cfg_text() -> String {
    let path = repo_root().join("cfg/xrpld-example.cfg");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn example_config_parses() {
    let text = example_cfg_text();
    Config::from_ini_str(&text).expect("example config should parse without error");
}

#[test]
fn example_config_spot_check_fields() {
    let text = example_cfg_text();
    let cfg = Config::from_ini_str(&text).expect("example config should parse");

    // [node_db] type=NuDB
    assert_eq!(cfg.node_db().kind, NodeDbKind::NuDb);
    // online_delete = 512
    assert_eq!(cfg.node_db().online_delete, Some(512));

    // [ssl_verify] 1
    assert!(cfg.ssl_verify());

    // [server] declares these three ports
    let server = cfg.server();
    assert!(server.port_names.contains(&"port_rpc_admin_local".to_owned()));
    assert!(server.port_names.contains(&"port_peer".to_owned()));
    assert!(server.port_names.contains(&"port_ws_admin_local".to_owned()));

    // [port_peer] port=51235
    let peer = cfg.port("port_peer").expect("port_peer missing");
    assert_eq!(peer.port, 51235);

    assert_eq!(cfg.network_id(), 0);
    assert_eq!(cfg.max_transactions(), 250);
}

#[test]
fn example_config_bootstrap_with_validators_file() {
    use std::fs;

    let tmp = std::env::temp_dir().join("config_integ_example_bootstrap");
    fs::create_dir_all(&tmp).expect("create temp dir");

    // Copy the example config
    let src_cfg = repo_root().join("cfg/xrpld-example.cfg");
    let dst_cfg = tmp.join("xrpld-example.cfg");
    fs::copy(&src_cfg, &dst_cfg)
        .unwrap_or_else(|e| panic!("copy config: {e}"));

    // Copy validators-example.txt as validators.txt (the name referenced in the config)
    let src_vl = repo_root().join("cfg/validators-example.txt");
    let dst_vl = tmp.join("validators.txt");
    fs::copy(&src_vl, &dst_vl)
        .unwrap_or_else(|e| panic!("copy validators: {e}"));

    let mut cfg = Config::from_file(&dst_cfg)
        .expect("from_file should succeed");
    cfg.set_standalone(true);
    cfg.set_quiet(true);
    cfg.bootstrap().expect("bootstrap should succeed");

    assert!(!cfg.validator_list_keys().is_empty(), "validator_list_keys should be spliced in");
    let _ = cfg.data_dir();
}

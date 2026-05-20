//! Integration tests: pair-wise INI vs TOML equivalence.
//!
//! For each matched pair (same logical config in both formats), parse both,
//! run bootstrap, and assert that key getters return identical values.
//!
//! Note on asymmetry: INI silently clamps out-of-range values; TOML errors.
//! Equivalence fixtures must therefore use in-range values only.

mod common;

use config::{NodeDbKind, SqliteMode, SqliteSafety};

// ---------------------------------------------------------------------------
// Pair 1: overlay settings
// ---------------------------------------------------------------------------

#[test]
fn equivalence_overlay() {
    let ini_text = r#"
[overlay]
max_unknown_time=900
max_diverged_time=120
ip_limit=3
"#;
    let toml_text = r#"
[overlay]
max_unknown_time = 900
max_diverged_time = 120
ip_limit = 3
"#;

    let ini_cfg = common::parse_ini_bootstrap(ini_text);
    let toml_cfg = common::parse_toml_bootstrap(toml_text);

    assert_eq!(ini_cfg.overlay().max_unknown_time, toml_cfg.overlay().max_unknown_time);
    assert_eq!(ini_cfg.overlay().max_diverged_time, toml_cfg.overlay().max_diverged_time);
    assert_eq!(ini_cfg.overlay().ip_limit, toml_cfg.overlay().ip_limit);
}

// ---------------------------------------------------------------------------
// Pair 2: node_db
// ---------------------------------------------------------------------------

#[test]
fn equivalence_node_db() {
    let ini_text = r#"
[node_db]
type=NuDB
path=/tmp/testdb
online_delete=512
earliest_seq=1
"#;
    let toml_text = r#"
[node_db]
kind = "NuDb"
path = "/tmp/testdb"
online_delete = 512
earliest_seq = 1
"#;

    let ini_cfg = common::parse_ini_bootstrap(ini_text);
    let toml_cfg = common::parse_toml_bootstrap(toml_text);

    assert_eq!(ini_cfg.node_db().kind, toml_cfg.node_db().kind);
    assert_eq!(ini_cfg.node_db().kind, NodeDbKind::NuDb);
    assert_eq!(ini_cfg.node_db().online_delete, toml_cfg.node_db().online_delete);
    assert_eq!(ini_cfg.node_db().earliest_seq, toml_cfg.node_db().earliest_seq);
    assert_eq!(
        ini_cfg.node_db().path.to_string_lossy(),
        toml_cfg.node_db().path.to_string_lossy()
    );
}

// ---------------------------------------------------------------------------
// Pair 3: sqlite safety level
// ---------------------------------------------------------------------------

#[test]
fn equivalence_sqlite_safety() {
    let ini_text = r#"
[sqlite]
safety_level=high
journal_size_limit=2000000
"#;
    let toml_text = r#"
[sqlite]
safety_level = "High"
journal_size_limit = 2000000
"#;

    let ini_cfg = common::parse_ini_bootstrap(ini_text);
    let toml_cfg = common::parse_toml_bootstrap(toml_text);

    assert!(
        matches!(ini_cfg.sqlite().mode, SqliteMode::Safety { level: SqliteSafety::High }),
        "INI: unexpected sqlite mode: {:?}", ini_cfg.sqlite().mode
    );
    assert!(
        matches!(toml_cfg.sqlite().mode, SqliteMode::Safety { level: SqliteSafety::High }),
        "TOML: unexpected sqlite mode: {:?}", toml_cfg.sqlite().mode
    );
    assert_eq!(ini_cfg.sqlite().journal_size_limit, toml_cfg.sqlite().journal_size_limit);
}

// ---------------------------------------------------------------------------
// Pair 4: top-level scalars (network_id, workers)
// ---------------------------------------------------------------------------

#[test]
fn equivalence_top_level_scalars() {
    let ini_text = r#"
[network_id]
1
[workers]
4
[compression]
1
"#;
    let toml_text = r#"
network_id = 1
workers = 4
compression = true
"#;

    let ini_cfg = common::parse_ini_bootstrap(ini_text);
    let toml_cfg = common::parse_toml_bootstrap(toml_text);

    assert_eq!(ini_cfg.network_id(), toml_cfg.network_id(), "network_id mismatch");
    assert_eq!(ini_cfg.workers(), toml_cfg.workers(), "workers mismatch");
    assert_eq!(ini_cfg.compression(), toml_cfg.compression(), "compression mismatch");
}

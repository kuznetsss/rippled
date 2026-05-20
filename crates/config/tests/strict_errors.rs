//! Integration tests: TOML inputs that SHOULD produce errors, asserted against
//! the expected `ConfigErrorKind`.
//!
//! These tests document the contract between TOML strict mode and operators:
//! each error case must produce the right variant so error messages are useful.

use config::{Config, ConfigErrorKind};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn parse_toml(text: &str) -> Result<Config, config::ConfigError> {
    Config::from_toml_str(text)
}

fn expect_err(text: &str) -> config::ConfigError {
    parse_toml(text).unwrap_err()
}

// ---------------------------------------------------------------------------
// 1. Unknown top-level key
//    toml crate wraps it as Grammar (deny_unknown_fields).
// ---------------------------------------------------------------------------

#[test]
fn strict_unknown_toplevel_key() {
    let err = expect_err("frobnicator = 42");
    // The toml crate wraps deny_unknown_fields failures as a Grammar error.
    assert!(
        matches!(err.kind, ConfigErrorKind::Grammar { .. }),
        "expected Grammar for unknown top-level key, got: {:?}",
        err.kind
    );
    let msg = err.to_string();
    assert!(
        msg.contains("unknown") || msg.contains("frobnicator") || msg.contains("TOML"),
        "message should mention the unknown key: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 2. Unknown key inside [overlay]
//    deny_unknown_fields on TomlOverlayConfig → Grammar
// ---------------------------------------------------------------------------

#[test]
fn strict_unknown_overlay_key() {
    let err = expect_err("[overlay]\nturbo_mode = true");
    assert!(
        matches!(err.kind, ConfigErrorKind::Grammar { .. }),
        "expected Grammar for unknown overlay key, got: {:?}",
        err.kind
    );
    let msg = err.to_string();
    assert!(
        msg.contains("unknown") || msg.contains("turbo_mode") || msg.contains("TOML"),
        "message should mention the problem: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 3. Out-of-range: overlay.max_unknown_time = 100 (min is 300)
// ---------------------------------------------------------------------------

#[test]
fn strict_overlay_max_unknown_time_out_of_range() {
    let err = expect_err("[overlay]\nmax_unknown_time = 100");
    assert!(
        matches!(err.kind, ConfigErrorKind::OutOfRange { ref field, .. } if field.contains("max_unknown_time")),
        "expected OutOfRange for max_unknown_time=100, got: {:?}",
        err.kind
    );
}

// ---------------------------------------------------------------------------
// 4. Mutual exclusion: [sqlite] safety_level + journal_mode
// ---------------------------------------------------------------------------

#[test]
fn strict_sqlite_mutual_exclusion() {
    let toml = "[sqlite]\nsafety_level = \"High\"\njournal_mode = \"Wal\"\n";
    let err = expect_err(toml);
    assert!(
        matches!(err.kind, ConfigErrorKind::MutualExclusion { .. }),
        "expected MutualExclusion for safety_level+journal_mode, got: {:?}",
        err.kind
    );
    let msg = err.to_string();
    assert!(
        msg.contains("safety_level"),
        "error message should mention safety_level: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 5. Orphan [port.foo] without server.ports = ["foo"]
// ---------------------------------------------------------------------------

#[test]
fn strict_orphan_port_table() {
    let toml = "[port.foo]\nport = 9999\nprotocol = [\"Http\"]\n";
    let err = expect_err(toml);
    assert!(
        matches!(err.kind, ConfigErrorKind::OrphanPortTable { ref name } if name == "foo"),
        "expected OrphanPortTable{{foo}}, got: {:?}",
        err.kind
    );
}

// ---------------------------------------------------------------------------
// 6. validation_seed + validator_token both set → MutualExclusion
// ---------------------------------------------------------------------------

#[test]
fn strict_validation_seed_and_token_mutual_exclusion() {
    let toml = "validation_seed = \"abc\"\nvalidator_token = \"xyz\"\n";
    let err = expect_err(toml);
    assert!(
        matches!(err.kind, ConfigErrorKind::MutualExclusion { ref first, ref second }
            if first.contains("validation_seed") && second.contains("validator_token")),
        "expected MutualExclusion for seed+token, got: {:?}",
        err.kind
    );
}

// ---------------------------------------------------------------------------
// 7. max_transactions = 50 in TOML → OutOfRange (INI would clamp silently)
// ---------------------------------------------------------------------------

#[test]
fn strict_max_transactions_too_low() {
    let err = expect_err("max_transactions = 50");
    assert!(
        matches!(err.kind, ConfigErrorKind::OutOfRange { ref field, value: 50, .. }
            if field.contains("max_transactions")),
        "expected OutOfRange for max_transactions=50, got: {:?}",
        err.kind
    );
}

// ---------------------------------------------------------------------------
// 8. max_transactions = 2000 (above 1000) → OutOfRange
// ---------------------------------------------------------------------------

#[test]
fn strict_max_transactions_too_high() {
    let err = expect_err("max_transactions = 2000");
    assert!(
        matches!(err.kind, ConfigErrorKind::OutOfRange { ref field, .. }
            if field.contains("max_transactions")),
        "expected OutOfRange for max_transactions=2000, got: {:?}",
        err.kind
    );
}

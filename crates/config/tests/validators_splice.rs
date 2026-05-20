//! Integration tests for the `validators.txt` splice behaviour in `bootstrap()`.
//!
//! Six scenarios:
//!   1. Main config + explicit `validators_file` pointing to a secondary file
//!      → validators from the secondary file are appended.
//!   2. Implicit `<config_dir>/validators.txt` discovered when `validators_file` is unset.
//!   3. Missing implicit `validators.txt` is silent (no error).
//!   4. Missing explicit `validators_file` is a bootstrap error.
//!   5. Overlap between main + secondary `[validators]` in INI: silent append.
//!   6. `[validator_list_keys]` from secondary file spliced into the main config.

use std::fs;
use std::path::PathBuf;

use config::{Config, ConfigErrorKind};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a temporary directory with a unique name derived from the test name.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("config_splice_{name}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn bootstrap(mut cfg: Config, config_dir: PathBuf) -> Result<Config, config::ConfigError> {
    cfg.set_config_dir(config_dir);
    cfg.set_standalone(true);
    cfg.set_quiet(true);
    cfg.bootstrap()?;
    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Scenario 1: explicit validators_file → validators appended
// ---------------------------------------------------------------------------

#[test]
fn splice_explicit_validators_file() {
    let dir = temp_dir("explicit");

    // Secondary file with one validator
    let secondary_path = dir.join("secondary_validators.cfg");
    fs::write(
        &secondary_path,
        "[validators]\nnHB5XIupk1A1YnRmHzZq2usDGECJd2MbMcKqfXbmCFCAmHbMRKqH Secondary\n",
    )
    .unwrap();

    // Main config references the secondary file (absolute path for simplicity)
    let ini = format!(
        "[validators]\nnHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu Main\n[validators_file]\n{}\n",
        secondary_path.display()
    );

    let cfg = Config::from_ini_str(&ini).unwrap();
    let cfg = bootstrap(cfg, dir).expect("bootstrap should succeed");

    let vs = cfg.trusted_validators();
    assert_eq!(vs.len(), 2, "expected 2 validators (1 main + 1 secondary), got {}", vs.len());

    let main_key = vs.iter().find(|v| v.key == "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu");
    assert!(main_key.is_some(), "main validator missing");
    assert_eq!(main_key.unwrap().label.as_deref(), Some("Main"));

    let sec_key = vs.iter().find(|v| v.key == "nHB5XIupk1A1YnRmHzZq2usDGECJd2MbMcKqfXbmCFCAmHbMRKqH");
    assert!(sec_key.is_some(), "secondary validator missing");
    assert_eq!(sec_key.unwrap().label.as_deref(), Some("Secondary"));
}

// ---------------------------------------------------------------------------
// Scenario 2: implicit <config_dir>/validators.txt is discovered
// ---------------------------------------------------------------------------

#[test]
fn splice_implicit_validators_txt_discovered() {
    let dir = temp_dir("implicit_found");

    // Write validators.txt in the config dir
    fs::write(
        dir.join("validators.txt"),
        "[validators]\nnHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK Implicit\n",
    )
    .unwrap();

    // Main config has no [validators_file] section
    let cfg = Config::from_ini_str("").unwrap();
    let cfg = bootstrap(cfg, dir).expect("bootstrap should succeed");

    let vs = cfg.trusted_validators();
    assert_eq!(vs.len(), 1, "expected 1 validator from implicit splice");
    assert_eq!(vs[0].key, "nHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK");
    assert_eq!(vs[0].label.as_deref(), Some("Implicit"));
}

// ---------------------------------------------------------------------------
// Scenario 3: missing implicit validators.txt is silent
// ---------------------------------------------------------------------------

#[test]
fn splice_missing_implicit_validators_txt_is_silent() {
    let dir = temp_dir("implicit_missing");
    // No validators.txt written to dir

    let cfg = Config::from_ini_str("").unwrap();
    let result = bootstrap(cfg, dir);
    assert!(result.is_ok(), "missing implicit validators.txt should be silent, got: {:?}", result.err());
}

// ---------------------------------------------------------------------------
// Scenario 4: missing explicit validators_file → bootstrap error
// ---------------------------------------------------------------------------

#[test]
fn splice_missing_explicit_validators_file_is_error() {
    let dir = temp_dir("explicit_missing");

    // Point validators_file at a path that doesn't exist
    let nonexistent = dir.join("does_not_exist.cfg");
    let ini = format!(
        "[validators_file]\n{}\n",
        nonexistent.display()
    );

    let cfg = Config::from_ini_str(&ini).unwrap();
    let result = bootstrap(cfg, dir);

    assert!(result.is_err(), "missing explicit validators_file should be a bootstrap error");
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, ConfigErrorKind::Io { .. }),
        "expected Io error, got: {:?}",
        err.kind
    );
}

// ---------------------------------------------------------------------------
// Scenario 5: overlap between main + secondary [validators] in INI → silent append
// ---------------------------------------------------------------------------

#[test]
fn splice_ini_overlap_is_silent_append() {
    let dir = temp_dir("overlap");

    let secondary_path = dir.join("validators.txt");
    // Secondary file has the same key as the main config
    fs::write(
        &secondary_path,
        "[validators]\nnHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu DuplicateLabel\n",
    )
    .unwrap();

    let ini = "[validators]\nnHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu Main\n";
    let cfg = Config::from_ini_str(ini).unwrap();
    let cfg = bootstrap(cfg, dir).expect("INI overlap should be silent append, not an error");

    // Both entries should be present (INI is lenient: silent append)
    let vs = cfg.trusted_validators();
    assert_eq!(vs.len(), 2, "expected 2 validators after silent append, got {}", vs.len());
    assert!(vs.iter().all(|v| v.key == "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu"));
}

// ---------------------------------------------------------------------------
// Scenario 6: validator_list_keys from secondary file spliced
// ---------------------------------------------------------------------------

#[test]
fn splice_validator_list_keys_from_secondary() {
    let dir = temp_dir("vl_keys");

    fs::write(
        dir.join("validators.txt"),
        "[validator_list_keys]\nED2677ABFFD1B33AC6FBC3062B71F1E8397C1505E1C42C64D11AD1B28FF73F4734\n",
    )
    .unwrap();

    let cfg = Config::from_ini_str("").unwrap();
    let cfg = bootstrap(cfg, dir).expect("bootstrap with validator_list_keys should succeed");

    let keys = cfg.validator_list_keys();
    assert_eq!(keys.len(), 1, "expected 1 validator_list_key, got {}", keys.len());
    assert_eq!(keys[0], "ED2677ABFFD1B33AC6FBC3062B71F1E8397C1505E1C42C64D11AD1B28FF73F4734");
}

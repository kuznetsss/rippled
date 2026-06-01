//! `impl Config` methods: normalise, validate, bootstrap, and apply_cli_flags.
//!
//! These live here rather than in `lib.rs` so that the implementation is
//! adjacent to the `Config` struct definition.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ConfigFormat;
use crate::cli_flags::CliFlags;
use crate::error::ParseError;
use crate::ini::parse_ini;
use crate::schema::database::NodeDb;
use crate::schema::enums::{LedgerHistory, LedgerHistoryName, StartUpType};
use crate::schema::server::Protocol;
use crate::schema::{Config, ValidatorData};

impl Config {
    /// Apply CLI flags to the config, translating them into `Config` fields.
    ///
    /// Mirrors the `vm.contains(...)` cascade in `Main.cpp` lines 573–768.
    /// Must be called before [`Config::normalize`].
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::Ini`] for invalid flag combinations:
    /// - `force_ledger_present_range` not exactly two comma-separated u32 values,
    ///   or `min > max`.
    /// - `trap_tx_hash` set without `replay`.
    /// - `net` combined with `load` or `replay` startup mode.
    /// - `quorum` set to `0`.
    pub(crate) fn apply_cli_flags(&mut self, cli: CliFlags) -> Result<(), ParseError> {
        // standalone
        self.standalone = cli.standalone;

        // force_ledger_present_range: parse "min,max"
        if let Some(raw) = &cli.force_ledger_present_range {
            let parts: Vec<&str> = raw.split(',').collect();
            if parts.len() != 2 {
                return Err(ParseError::Ini(
                    "invalid 'force_ledger_present_range' parameter. The parameter must be \
                     two numbers separated by a comma. The first number must be <= the second."
                        .into(),
                ));
            }
            let min: u32 = parts[0].trim().parse().map_err(|_| {
                ParseError::Ini(
                    "invalid 'force_ledger_present_range' parameter. The parameter must be \
                     two numbers separated by a comma. The first number must be <= the second."
                        .into(),
                )
            })?;
            let max: u32 = parts[1].trim().parse().map_err(|_| {
                ParseError::Ini(
                    "invalid 'force_ledger_present_range' parameter. The parameter must be \
                     two numbers separated by a comma. The first number must be <= the second."
                        .into(),
                )
            })?;
            if min > max {
                return Err(ParseError::Ini(
                    "invalid 'force_ledger_present_range' parameter. The parameter must be \
                     two numbers separated by a comma. The first number must be <= the second."
                        .into(),
                ));
            }
            self.forced_ledger_range_present = Some((min, max));
        }

        // start → Fresh
        if cli.start {
            self.start_up = StartUpType::Fresh;
        }

        // import
        if cli.import {
            self.do_import = true;
        }

        // Determine the fast_load flag from node_db (mirrors C++ `config->FAST_LOAD`)
        let fast_load = match &self.node_db {
            Some(NodeDb::NuDb(opts)) => opts.common.fast_load.unwrap_or(false),
            Some(NodeDb::RocksDb(opts)) => opts.common.fast_load.unwrap_or(false),
            None => false,
        };

        // ledger / ledgerfile / load
        if let Some(ledger) = cli.ledger {
            self.start_ledger = Some(ledger);
            if cli.replay {
                self.start_up = StartUpType::Replay;
                if let Some(ref hash) = cli.trap_tx_hash {
                    self.trap_tx_hash = Some(hash.clone());
                }
            } else {
                self.start_up = StartUpType::Load;
            }
        } else if let Some(ledger_file) = cli.ledger_file {
            self.start_ledger = Some(ledger_file);
            self.start_up = StartUpType::LoadFile;
        } else if cli.load || fast_load {
            self.start_up = StartUpType::Load;
        }

        // trap_tx_hash without replay → error
        if cli.trap_tx_hash.is_some() && !cli.replay {
            return Err(ParseError::Ini(
                "Cannot use trap option without replay option".into(),
            ));
        }

        // net (and NOT fast_load)
        if cli.net && !fast_load {
            if self.start_up == StartUpType::Load || self.start_up == StartUpType::Replay {
                return Err(ParseError::Ini(
                    "Net and load/replay options are incompatible".into(),
                ));
            }
            self.start_up = StartUpType::Network;
        }

        // valid
        if cli.valid {
            self.start_valid = true;
        }

        // rpc_ip: store raw string; full endpoint parsing stays C++-side
        if let Some(ip) = &cli.rpc_ip {
            self.rpc_ip = Some(ip.clone());
        }

        // quorum: 0 is invalid
        if let Some(q) = cli.quorum {
            if q == 0 {
                return Err(ParseError::Ini(
                    "Invalid value specified for --quorum".into(),
                ));
            }
            self.network_quorum = Some(q);
        }

        Ok(())
    }

    /// Perform all post-parse normalization.
    ///
    /// Must be called after [`Config::apply_cli_flags`] so that
    /// `self.standalone` is already set when validators-file loading is gated.
    ///
    /// Responsibilities (in order):
    /// 1. Load + merge `validators_file` (if set and NOT standalone).
    /// 2. Absolutize `validators_file` path (when `config_dir` is `Some`).
    /// 3. Consolidate `validator_keys` → `validators`.
    /// 4. validator_list_threshold == 0 → `None` (C++ treats 0 as "auto").
    /// 5. Apply `self.standalone` → force `ledger_history = 0`.
    /// 6. Set `path_search_max = 0` when `validation_seed` or `validator_token`
    ///    is set (mirrors C++ §3.3 rule 15).
    /// 7. Absolutize `database_path` and `debug_logfile` against `config_dir`.
    pub(crate) fn normalize(&mut self, config_dir: Option<&Path>) -> Result<(), ParseError> {
        // 1 & 2: validators_file load + merge + absolutize
        // Skipped in standalone mode (mirrors C++ `load()` behaviour).
        if !self.standalone
            && let Some(validators_path) = self.validators_file.take()
        {
            let validators_path = absolutize_path(&validators_path, config_dir);

            let v_contents = fs::read_to_string(&validators_path)?;

            let (validator_data, strict) = match ConfigFormat::from_path(&validators_path)? {
                ConfigFormat::Toml => (toml::from_str::<ValidatorData>(&v_contents)?, true),
                ConfigFormat::Ini => {
                    let bc = parse_ini(&v_contents);
                    (crate::ini::validators_from_basic_config(&bc)?, false)
                }
            };

            self.validators_file = Some(validators_path);
            self.merge_validators(validator_data, strict)?;
        }

        // 3: Consolidate validator_keys → validators (mirrors C++ §3.3 rule 9)
        let extra = self.validator_keys.clone();
        self.validators.extend(extra);

        // 4: validator_list_threshold == 0 → None (C++ treats 0 as disabled)
        if self.validator_list_threshold == Some(0) {
            self.validator_list_threshold = None;
        }

        // 5: standalone → force ledger_history = 0
        if self.standalone {
            self.ledger_history = Some(LedgerHistory::Numeric(0));
        }

        // 6: validation_seed or validator_token → PATH_SEARCH_MAX = 0
        // Only set to 0 if not already explicitly set in the config.
        if (self.validation_seed.is_some() || self.validator_token.is_some())
            && self.path_search_max.is_none()
        {
            self.path_search_max = Some(0);
        }

        // 7: absolutize database_path and debug_logfile
        if let Some(ref p) = self.database_path.clone() {
            self.database_path = Some(absolutize_path(p, config_dir));
        }
        if let Some(ref p) = self.debug_logfile.clone() {
            self.debug_logfile = Some(absolutize_path(p, config_dir));
        }
        if let Some(ref p) = self.ssl_verify_file.clone() {
            self.ssl_verify_file = Some(absolutize_path(p, config_dir));
        }
        if let Some(ref p) = self.ssl_verify_dir.clone() {
            self.ssl_verify_dir = Some(absolutize_path(p, config_dir));
        }

        Ok(())
    }

    /// Cross-section validation.  Called after [`Config::normalize`] so it sees
    /// the post-override state.  Mirrors the §5 rules from `config_schema.md`
    /// plus the checks that C++ `Config::load` / `checkZeroPorts` perform.
    pub(crate) fn validate(&self) -> Result<(), ParseError> {
        // ---- validator_list_threshold ----------------------------------------
        if let Some(threshold) = self.validator_list_threshold {
            // threshold == 0 was already converted to None in normalise; so here
            // any Some value must be >= 1.
            if threshold as usize > self.validator_list_keys.len() {
                return Err(ParseError::Ini(format!(
                    "validator_list_threshold ({threshold}) exceeds the number of \
                     validator_list_keys ({})",
                    self.validator_list_keys.len()
                )));
            }
        }

        // ---- validator_list_sites requires validator_list_keys ---------------
        if !self.validator_list_sites.is_empty() && self.validator_list_keys.is_empty() {
            return Err(ParseError::Ini(
                "validator_list_sites requires validator_list_keys to be non-empty".into(),
            ));
        }

        // ---- validation_seed / validator_token mutex -------------------------
        if self.validation_seed.is_some() && self.validator_token.is_some() {
            return Err(ParseError::Ini(
                "validation_seed and validator_token cannot both be set".into(),
            ));
        }

        // ---- peers_in_max / peers_out_max togetherness -----------------------
        // Both must be set together; ignored if peers_max is set.
        if self.peers_max.is_none() {
            match (self.peers_in_max, self.peers_out_max) {
                (Some(_), None) => {
                    return Err(ParseError::Ini(
                        "peers_in_max requires peers_out_max to also be set".into(),
                    ));
                }
                (None, Some(_)) => {
                    return Err(ParseError::Ini(
                        "peers_out_max requires peers_in_max to also be set".into(),
                    ));
                }
                _ => {}
            }
        }

        // ---- network_quorum <= effective peers_max ---------------------------
        // C++: error if network_quorum > peers_max (or 21 when peers_max == 0).
        if let Some(quorum) = self.network_quorum {
            let effective_max = self.peers_max.unwrap_or(21);
            if effective_max != 0 && quorum > effective_max {
                return Err(ParseError::Ini(format!(
                    "network_quorum ({quorum}) must not exceed peers_max ({effective_max})"
                )));
            }
        }

        // ---- node_db.online_delete >= ledger_history -------------------------
        let ledger_history_numeric = self.ledger_history.and_then(|lh| match lh {
            LedgerHistory::Numeric(n) => Some(n),
            LedgerHistory::Named(LedgerHistoryName::None) => Some(0),
            LedgerHistory::Named(LedgerHistoryName::Full) => None, // unlimited → skip check
        });
        if let Some(lh) = ledger_history_numeric
            && lh > 0
            && let Some(ref node_db) = self.node_db
        {
            let online_delete = match node_db {
                NodeDb::NuDb(opts) => opts.common.online_delete,
                NodeDb::RocksDb(opts) => opts.common.online_delete,
            };
            if let Some(od) = online_delete
                && od > 0
                && od < lh
            {
                return Err(ParseError::Ini(format!(
                    "node_db.online_delete ({od}) must be >= ledger_history ({lh})"
                )));
            }
        }

        // ---- sqlite.safety_level mutex with journal_mode/synchronous/temp_store
        if let Some(ref sqlite) = self.sqlite
            && sqlite.safety_level.is_some()
            && (sqlite.journal_mode.is_some()
                || sqlite.synchronous.is_some()
                || sqlite.temp_store.is_some())
        {
            return Err(ParseError::Ini(
                "sqlite.safety_level cannot be set together with \
                 journal_mode, synchronous, or temp_store"
                    .into(),
            ));
        }

        // ---- transaction_queue.maximum_txn_in_ledger >= both minimums --------
        if let Some(ref tq) = self.transaction_queue
            && let Some(max) = tq.maximum_txn_in_ledger
        {
            let min_net = tq.minimum_txn_in_ledger.unwrap_or(5);
            let min_sa = tq.minimum_txn_in_ledger_standalone.unwrap_or(1000);
            if max < min_net {
                return Err(ParseError::Ini(format!(
                    "transaction_queue.maximum_txn_in_ledger ({max}) must be >= \
                     minimum_txn_in_ledger ({min_net})"
                )));
            }
            if max < min_sa {
                return Err(ParseError::Ini(format!(
                    "transaction_queue.maximum_txn_in_ledger ({max}) must be >= \
                     minimum_txn_in_ledger_standalone ({min_sa})"
                )));
            }
        }

        // ---- reduce_relay: vp_base_squelch_enable vs vp_enable mutex ---------
        if let Some(ref rr) = self.reduce_relay
            && rr.vp_base_squelch_enable.is_some()
            && rr.vp_enable.is_some()
        {
            return Err(ParseError::Ini(
                "reduce_relay.vp_base_squelch_enable and vp_enable cannot both be set \
                 (vp_enable is a deprecated alias)"
                    .into(),
            ));
        }

        // ---- hashrouter: relay_time <= hold_time ----------------------------
        if let Some(ref hr) = self.hashrouter
            && let (Some(relay), Some(hold)) = (hr.relay_time, hr.hold_time)
            && relay > hold
        {
            return Err(ParseError::Ini(format!(
                "hashrouter.relay_time ({relay}) must be <= hold_time ({hold})"
            )));
        }

        // ---- grpc: ssl_cert / ssl_key togetherness ---------------------------
        if let Some(ref grpc) = self.grpc {
            let has_cert = grpc.ssl_cert.is_some();
            let has_key = grpc.ssl_key.is_some();
            if has_cert != has_key {
                return Err(ParseError::Ini(
                    "grpc ssl_cert and ssl_key must both be set or both unset".into(),
                ));
            }
            // ssl_cert_chain and ssl_client_ca require both cert+key
            let has_both = has_cert && has_key;
            if !has_both && (grpc.ssl_cert_chain.is_some() || grpc.ssl_client_ca.is_some()) {
                return Err(ParseError::Ini(
                    "grpc ssl_cert_chain and ssl_client_ca require ssl_cert and ssl_key".into(),
                ));
            }
        }

        // ---- [server] validation --------------------------------------------
        if let Some(ref server) = self.server {
            // port = 0 is rejected (checkZeroPorts equivalent).
            // Note: an empty ports map is allowed here — C++ validates the
            // "Required section" constraint at the parsePorts level, which is
            // a separate pass not replicated in Rust's validate().
            for (name, port_cfg) in &server.ports {
                if port_cfg.port == Some(0) {
                    return Err(ParseError::Ini(format!(
                        "port = 0 is not allowed in server port section [{name}]"
                    )));
                }
            }

            // In network mode: at most one port may include 'peer' protocol
            let peer_count = server
                .ports
                .values()
                .filter(|p| {
                    p.protocol
                        .as_deref()
                        .unwrap_or(&[])
                        .contains(&Protocol::Peer)
                })
                .count();
            if peer_count > 1 {
                return Err(ParseError::Ini(format!(
                    "at most one port may use the 'peer' protocol, found {peer_count}"
                )));
            }
        }

        Ok(())
    }

    /// Create directories that xrpld needs at runtime.
    ///
    /// Called by C++ after deciding to actually run the node (not for
    /// `--vacuum`, `--standalone`-lint, etc.).  Uses the absolutized paths
    /// already populated by [`Config::normalize`].
    ///
    /// * `database_path` — created via `fs::create_dir_all`.
    /// * `debug_logfile` parent — created via `fs::create_dir_all`.
    ///
    /// SSL context init is intentionally excluded — that stays in C++.
    ///
    /// Returns `Err(String)` on I/O failure (cxx-compatible error type).
    pub fn bootstrap(&self) -> Result<(), String> {
        if let Some(ref db_path) = self.database_path {
            fs::create_dir_all(db_path).map_err(|e| {
                format!(
                    "bootstrap: failed to create database_path {}: {e}",
                    db_path.display()
                )
            })?;
        }
        if let Some(ref log_file) = self.debug_logfile
            && let Some(parent) = log_file.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "bootstrap: failed to create log directory {}: {e}",
                    parent.display()
                )
            })?;
        }
        Ok(())
    }
}

/// Resolve `p` against `config_dir`.  If `p` is already absolute, or if
/// `config_dir` is `None`, `p` is returned unchanged.
pub(crate) fn absolutize_path(p: &Path, config_dir: Option<&Path>) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        match config_dir {
            Some(dir) => dir.join(p),
            None => p.to_path_buf(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests for normalize, validate, and bootstrap
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::error::ParseError;
    use crate::loader::parse_from_toml_str;
    use crate::schema::Config;
    use crate::schema::enums::{LedgerHistory, StartUpType};
    use crate::{ConfigFormat, parse_from_file, parse_from_str};

    /// Helper: parse TOML, apply no CLI flags, finalize.
    fn parse_toml(s: &str) -> Result<Config, ParseError> {
        let (cfg, _warnings) = parse_from_str(s, ConfigFormat::Toml)?.finalize()?;
        Ok(cfg)
    }

    /// Helper: parse from str and finalize with default (empty) CLI flags.
    fn parse_str_finalize(
        s: &str,
        fmt: ConfigFormat,
    ) -> Result<(Config, crate::loader::IniWarnings), ParseError> {
        parse_from_str(s, fmt)?.finalize()
    }

    #[test]
    fn parse_from_toml_internal_minimal() {
        let cfg = parse_from_toml_str("network_quorum = 3").unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
    }

    #[test]
    fn parse_from_str_toml_works() {
        let (cfg, warnings) = parse_str_finalize("network_quorum = 3", ConfigFormat::Toml).unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
        assert!(!warnings.had_trailing_comments);
    }

    #[test]
    fn parse_from_str_ini_works() {
        let (cfg, warnings) = parse_str_finalize("[network_quorum]\n3", ConfigFormat::Ini).unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
        assert!(!warnings.had_trailing_comments);
    }

    #[test]
    fn parse_from_toml_str_surfaces_parse_errors() {
        let err = parse_from_toml_str("not_a_real_key = 1").unwrap_err();
        assert!(matches!(err, ParseError::Toml(_)), "got {err:?}");
    }

    #[test]
    fn parse_from_file_dispatches_by_extension() {
        let dir = std::env::temp_dir().join(format!("config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("example.toml");
        std::fs::write(&toml_path, "network_quorum = 7\n").unwrap();

        let (cfg, warnings) = parse_from_file(&toml_path).unwrap().finalize().unwrap();
        assert_eq!(cfg.network_quorum, Some(7));
        assert!(!warnings.had_trailing_comments);

        std::fs::remove_file(&toml_path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_file_ini_extension() {
        let dir = std::env::temp_dir().join(format!("config-test-ini-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini_path = dir.join("example.cfg");
        std::fs::write(&ini_path, "[network_quorum]\n3\n").unwrap();

        let (cfg, _) = parse_from_file(&ini_path).unwrap().finalize().unwrap();
        assert_eq!(cfg.network_quorum, Some(3));

        std::fs::remove_file(&ini_path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_file_io_error_is_typed() {
        let err = parse_from_file("/nonexistent/path/to/xrpld.toml").unwrap_err();
        assert!(matches!(err, ParseError::Io(_)), "got {err:?}");
    }

    #[test]
    fn parse_from_file_unsupported_extension_errors() {
        let dir = std::env::temp_dir().join(format!("config-test-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("xrpld.yaml");
        std::fs::write(&path, "").unwrap();

        let err = parse_from_file(&path).unwrap_err();
        assert!(
            matches!(err, ParseError::UnsupportedFormat(ref ext) if ext == "yaml"),
            "got {err:?}",
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_ini_str_minimal() {
        let (cfg, warnings) = parse_str_finalize("[network_quorum]\n3", ConfigFormat::Ini).unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
        assert!(!warnings.had_trailing_comments);
    }

    #[test]
    fn parse_from_ini_str_returns_ini_warnings() {
        // A trailing comment should surface in IniWarnings.
        let (_, warnings) =
            parse_str_finalize("[network_quorum]\n3 # trailing", ConfigFormat::Ini).unwrap();
        assert!(warnings.had_trailing_comments);
    }

    // -----------------------------------------------------------------------
    // Fix 1: validator_keys entries are consolidated into validators
    // -----------------------------------------------------------------------

    #[test]
    fn fix1_validator_keys_consolidated_into_validators_no_validators_file() {
        let dir = std::env::temp_dir().join(format!("config-fix1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // Main config has entries in both [validators] and [validator_keys].
        // [validator_list_keys] is provided so threshold validation can pass.
        std::fs::write(
            &cfg_path,
            "[validators]\nnVAL1\n\n[validator_keys]\nnKEY1\nnKEY2\n\
             \n[validator_list_keys]\nhexkey1\nhexkey2\nhexkey3\n",
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap().finalize().unwrap();
        // After consolidation, validators must include the original entry
        // plus the two from validator_keys.
        assert!(
            cfg.validators.contains(&"nVAL1".to_string()),
            "original validator missing"
        );
        assert!(
            cfg.validators.contains(&"nKEY1".to_string()),
            "nKEY1 from validator_keys missing"
        );
        assert!(
            cfg.validators.contains(&"nKEY2".to_string()),
            "nKEY2 from validator_keys missing"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // Fix 2: validators file threshold wins over main config threshold
    // -----------------------------------------------------------------------

    #[test]
    fn fix2_validators_file_threshold_wins_over_main_config() {
        let dir = std::env::temp_dir().join(format!("config-fix2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Validators file: threshold=2, two list keys.
        let vfile_path = dir.join("validators.txt");
        std::fs::write(
            &vfile_path,
            "[validator_list_keys]\nhexkey1\nhexkey2\n\
             \n[validator_list_threshold]\n2\n",
        )
        .unwrap();

        // Main config: also sets threshold=5 (should be overwritten by file).
        let cfg_path = dir.join("xrpld.cfg");
        std::fs::write(
            &cfg_path,
            format!(
                "[validators_file]\n{}\n\n[validator_list_threshold]\n5\n",
                vfile_path.display()
            ),
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap().finalize().unwrap();
        // The validators file threshold (2) must win.
        assert_eq!(
            cfg.validator_list_threshold,
            Some(2),
            "validators file threshold should overwrite main config threshold"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // Fix 3: validator_list_threshold validation
    // -----------------------------------------------------------------------

    #[test]
    fn fix3_threshold_zero_is_none_after_normalise() {
        // C++ treats threshold == 0 as "auto" (None). normalise converts it.
        let dir = std::env::temp_dir().join(format!("config-fix3a-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // 1 key, threshold=0 → should become None (not an error)
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\n\n[validator_list_threshold]\n0\n",
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap().finalize().unwrap();
        assert_eq!(
            cfg.validator_list_threshold, None,
            "threshold 0 should become None"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fix3_threshold_exceeds_keys_count_is_error() {
        let dir = std::env::temp_dir().join(format!("config-fix3b-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // 1 key but threshold=5.
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\n\n[validator_list_threshold]\n5\n",
        )
        .unwrap();

        let err = parse_from_file(&cfg_path).unwrap().finalize().unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("exceeds")),
            "expected threshold>keys error, got: {err:?}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fix3_valid_threshold_succeeds() {
        let dir = std::env::temp_dir().join(format!("config-fix3c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // 3 keys, threshold=2: valid.
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\nhexkey2\nhexkey3\n\
             \n[validator_list_threshold]\n2\n",
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap().finalize().unwrap();
        assert_eq!(cfg.validator_list_threshold, Some(2));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // CLI flags: standalone forces ledger_history = 0
    // -----------------------------------------------------------------------

    #[test]
    fn standalone_forces_ledger_history_zero() {
        let mut builder = parse_from_str(r#"ledger_history = "full""#, ConfigFormat::Toml).unwrap();
        builder.set_standalone(true);
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(
            cfg.ledger_history,
            Some(LedgerHistory::Numeric(0)),
            "standalone must force ledger_history = 0"
        );
    }

    #[test]
    fn quorum_override_replaces_config_value() {
        let mut builder = parse_from_str("network_quorum = 3", ConfigFormat::Toml).unwrap();
        builder.set_quorum(7);
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.network_quorum, Some(7));
    }

    #[test]
    fn validator_token_sets_path_search_max_zero() {
        let (cfg, _) =
            parse_str_finalize(r#"validator_token = "sometoken""#, ConfigFormat::Toml).unwrap();
        assert_eq!(
            cfg.path_search_max,
            Some(0),
            "path_search_max must be 0 when validator_token is set"
        );
    }

    // -----------------------------------------------------------------------
    // apply_cli_flags: start_up types
    // -----------------------------------------------------------------------

    #[test]
    fn cli_start_flag_sets_fresh() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_start(true);
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.start_up, StartUpType::Fresh);
    }

    #[test]
    fn cli_ledger_without_replay_sets_load() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_ledger("abc123");
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.start_up, StartUpType::Load);
        assert_eq!(cfg.start_ledger.as_deref(), Some("abc123"));
    }

    #[test]
    fn cli_ledger_with_replay_sets_replay() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_ledger("abc123");
        builder.set_replay(true);
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.start_up, StartUpType::Replay);
    }

    #[test]
    fn cli_trap_without_replay_is_error() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_trap_tx_hash("deadbeef");
        let err = builder.finalize().unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("trap")),
            "got {err:?}"
        );
    }

    #[test]
    fn cli_net_with_load_is_error() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_load(true);
        builder.set_net(true);
        let err = builder.finalize().unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("incompatible")),
            "got {err:?}"
        );
    }

    #[test]
    fn cli_quorum_zero_is_error() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_quorum(0);
        let err = builder.finalize().unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("quorum")),
            "got {err:?}"
        );
    }

    #[test]
    fn cli_force_ledger_range_invalid_format_is_error() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_force_ledger_present_range("100");
        let err = builder.finalize().unwrap_err();
        assert!(matches!(&err, ParseError::Ini(_)), "got {err:?}");
    }

    #[test]
    fn cli_force_ledger_range_min_gt_max_is_error() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_force_ledger_present_range("200,100");
        let err = builder.finalize().unwrap_err();
        assert!(matches!(&err, ParseError::Ini(_)), "got {err:?}");
    }

    #[test]
    fn cli_force_ledger_range_valid() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_force_ledger_present_range("100,200");
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.forced_ledger_range_present, Some((100, 200)));
    }

    #[test]
    fn cli_import_sets_do_import() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_import(true);
        let (cfg, _) = builder.finalize().unwrap();
        assert!(cfg.do_import);
    }

    #[test]
    fn cli_valid_sets_start_valid() {
        let mut builder = parse_from_str("", ConfigFormat::Toml).unwrap();
        builder.set_valid(true);
        let (cfg, _) = builder.finalize().unwrap();
        assert!(cfg.start_valid);
    }

    // -----------------------------------------------------------------------
    // validate: validation_seed + validator_token mutex
    // -----------------------------------------------------------------------

    #[test]
    fn validate_seed_and_token_together_is_error() {
        let err = parse_toml(
            r#"
            validation_seed = "sseed"
            validator_token = "stoken"
            "#,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("validation_seed") && msg.contains("validator_token")),
            "got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // validate: sqlite safety_level mutex
    // -----------------------------------------------------------------------

    #[test]
    fn validate_sqlite_safety_level_mutex() {
        let err = parse_toml(
            r#"
            [sqlite]
            safety_level = "high"
            journal_mode = "wal"
            "#,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("safety_level")),
            "got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // validate: peers_in_max / peers_out_max togetherness
    // -----------------------------------------------------------------------

    #[test]
    fn validate_peers_in_without_out_is_error() {
        let err = parse_toml("peers_in_max = 50").unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("peers_in_max")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_peers_out_without_in_is_error() {
        let err = parse_toml("peers_out_max = 50").unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("peers_out_max")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_peers_in_out_together_ok() {
        let cfg = parse_toml("peers_in_max = 50\npeers_out_max = 10").unwrap();
        assert_eq!(cfg.peers_in_max, Some(50));
        assert_eq!(cfg.peers_out_max, Some(10));
    }

    // -----------------------------------------------------------------------
    // bootstrap: directory creation
    // -----------------------------------------------------------------------

    #[test]
    fn bootstrap_creates_database_path() {
        let base = std::env::temp_dir().join(format!("config-bootstrap-db-{}", std::process::id()));
        let cfg_toml = format!(r#"database_path = "{}""#, base.display());
        let (cfg, _) = parse_str_finalize(&cfg_toml, ConfigFormat::Toml).unwrap();

        assert!(!base.exists(), "dir should not exist yet");
        cfg.bootstrap().expect("bootstrap should succeed");
        assert!(base.is_dir(), "bootstrap must create database_path");

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn bootstrap_creates_debug_logfile_parent() {
        let base =
            std::env::temp_dir().join(format!("config-bootstrap-log-{}", std::process::id()));
        let log_path = base.join("logs").join("debug.log");
        let cfg_toml = format!(r#"debug_logfile = "{}""#, log_path.display());
        let (cfg, _) = parse_str_finalize(&cfg_toml, ConfigFormat::Toml).unwrap();

        assert!(!base.exists(), "dir should not exist yet");
        cfg.bootstrap().expect("bootstrap should succeed");
        assert!(
            log_path.parent().unwrap().is_dir(),
            "bootstrap must create log parent"
        );

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn bootstrap_no_paths_is_noop() {
        let (cfg, _) = parse_str_finalize("network_quorum = 3", ConfigFormat::Toml).unwrap();
        cfg.bootstrap()
            .expect("bootstrap with no paths must succeed");
    }
}

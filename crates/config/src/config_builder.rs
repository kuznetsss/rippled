//! `ConfigBuilder` — holds a parsed `Config` and its build context until
//! `finalize()` is called.
//!
//! The two-stage flow:
//! 1. `parse_from_str` / `parse_from_file` → `ConfigBuilder`
//! 2. (optionally) set CLI flags on the builder
//! 3. `ConfigBuilder::finalize()` → `(Config, IniWarnings)`

use std::path::PathBuf;

use crate::cli_flags::CliFlags;
use crate::error::ParseError;
use crate::loader::IniWarnings;
use crate::schema::Config;

/// Holds a freshly parsed (but not yet finalized) configuration.
///
/// Callers set CLI flags via the `set_*` methods, then call [`finalize`] to
/// run `apply_cli_flags → normalize → validate` and obtain the final `Config`.
#[derive(Debug)]
pub struct ConfigBuilder {
    config: Config,
    cli: CliFlags,
    config_dir: Option<PathBuf>,
    warnings: IniWarnings,
}

impl ConfigBuilder {
    /// Create a new builder wrapping a freshly parsed config.
    pub(crate) fn new(config: Config, config_dir: Option<PathBuf>, warnings: IniWarnings) -> Self {
        Self {
            config,
            cli: CliFlags::default(),
            config_dir,
            warnings,
        }
    }

    // ---- CLI flag setters (forwarded to the inner CliFlags) ----

    /// Set `--standalone` / `-a`.
    pub fn set_standalone(&mut self, value: bool) {
        self.cli.set_standalone(value);
    }

    /// Set `--quiet`.
    pub fn set_quiet(&mut self, value: bool) {
        self.cli.set_quiet(value);
    }

    /// Set `--silent`.
    pub fn set_silent(&mut self, value: bool) {
        self.cli.set_silent(value);
    }

    /// Set `--quorum <n>`.
    pub fn set_quorum(&mut self, value: u32) {
        self.cli.set_quorum(value);
    }

    /// Set `--start`.
    pub fn set_start(&mut self, value: bool) {
        self.cli.set_start(value);
    }

    /// Set `--ledger <hash-or-seq>`.
    pub fn set_ledger(&mut self, value: &str) {
        self.cli.set_ledger(value);
    }

    /// Set `--ledgerfile <path>`.
    pub fn set_ledgerfile(&mut self, value: &str) {
        self.cli.set_ledgerfile(value);
    }

    /// Set `--load`.
    pub fn set_load(&mut self, value: bool) {
        self.cli.set_load(value);
    }

    /// Set `--net`.
    pub fn set_net(&mut self, value: bool) {
        self.cli.set_net(value);
    }

    /// Set `--replay`.
    pub fn set_replay(&mut self, value: bool) {
        self.cli.set_replay(value);
    }

    /// Set `--trap_tx_hash <hash>`.
    pub fn set_trap_tx_hash(&mut self, value: &str) {
        self.cli.set_trap_tx_hash(value);
    }

    /// Set `--valid`.
    pub fn set_valid(&mut self, value: bool) {
        self.cli.set_valid(value);
    }

    /// Set `--import`.
    pub fn set_import(&mut self, value: bool) {
        self.cli.set_import(value);
    }

    /// Set `--force_ledger_present_range <min,max>`.
    pub fn set_force_ledger_present_range(&mut self, value: &str) {
        self.cli.set_force_ledger_present_range(value);
    }

    /// Set `--rpc_ip <endpoint>`.
    pub fn set_rpc_ip(&mut self, value: &str) {
        self.cli.set_rpc_ip(value);
    }

    /// Set `--rpc_port <port>`.
    pub fn set_rpc_port(&mut self, value: u16) {
        self.cli.set_rpc_port(value);
    }

    /// Set `--nodeid <id>`.
    pub fn set_nodeid(&mut self, value: &str) {
        self.cli.set_nodeid(value);
    }

    /// Set `--newnodeid`.
    pub fn set_newnodeid(&mut self, value: bool) {
        self.cli.set_newnodeid(value);
    }

    // ---- Inspection ----

    /// Returns `true` if the source was an INI file with trailing comments.
    /// Always `false` for TOML sources and for error results.
    pub fn had_trailing_comments(&self) -> bool {
        self.warnings.had_trailing_comments
    }

    // ---- Finalization ----

    /// Apply CLI flags, normalize, and validate the config.
    ///
    /// Consumes the builder and returns `(Config, IniWarnings)` on success.
    pub fn finalize(mut self) -> Result<(Config, IniWarnings), ParseError> {
        self.config.apply_cli_flags(self.cli)?;
        self.config.normalize(self.config_dir.as_deref())?;
        self.config.validate()?;
        Ok((self.config, self.warnings))
    }
}

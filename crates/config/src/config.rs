use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::ConfigError;
use crate::types::*;

// ---------------------------------------------------------------------------
// Internal bucket structs — pub(crate) so INI/TOML format modules can write
// into them without going through the public Config API.
// ---------------------------------------------------------------------------

/// All values derived from the config file. Immutable after parsing.
/// Field names match TOML/INI section/key names 1:1 so the TOML `From<Root>`
/// conversion is a straight move with no renaming.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct Parsed {
    /// Which format produced this `Parsed`. Used by bootstrap to enforce format-specific
    /// rules (e.g. validators-file overlap is a TOML error, silent in INI).
    pub(crate) source_format: crate::error::Format,
    // ---- top-level scalars ----
    pub(crate) network_id: u32,
    pub(crate) network_quorum: u64,
    pub(crate) peer_private: bool,
    pub(crate) peers_max: u32,
    pub(crate) peers_in_max: u32,
    pub(crate) peers_out_max: u32,
    pub(crate) relay_untrusted_validations: RelayPolicy,
    pub(crate) relay_untrusted_proposals: RelayPolicy,
    pub(crate) node_size: Option<NodeSize>,
    pub(crate) signing_enabled: bool,
    pub(crate) elb_support: bool,
    pub(crate) ssl_verify: bool,
    pub(crate) ssl_verify_file: Option<PathBuf>,
    pub(crate) ssl_verify_dir: Option<PathBuf>,
    pub(crate) ledger_history: LedgerHistory,
    pub(crate) fetch_depth: FetchDepth,
    pub(crate) path_search_old: i32,
    pub(crate) path_search: i32,
    pub(crate) path_search_fast: i32,
    pub(crate) path_search_max: i32,
    pub(crate) max_transactions: i32,
    pub(crate) amendment_majority_time: Duration,
    pub(crate) workers: u32,
    pub(crate) io_workers: u32,
    pub(crate) prefetch_workers: u32,
    pub(crate) sweep_interval: Option<u32>,
    pub(crate) compression: bool,
    pub(crate) ledger_replay: bool,
    pub(crate) beta_rpc_api: bool,
    pub(crate) server_domain: Option<String>,
    pub(crate) validator_list_threshold: Option<u64>,
    pub(crate) websocket_ping_frequency: Option<u32>,

    // ---- path fields (resolved during bootstrap) ----
    /// Resolved relative to config dir during bootstrap.
    pub(crate) debug_logfile: Option<RelPath>,
    /// Default: `<config_dir>/db`; overridden by `[database_path]`.
    pub(crate) database_path: Option<RelPath>,
    /// Resolved relative to config dir during bootstrap.
    pub(crate) validators_file: Option<RelPath>,

    // ---- fee / voting ----
    pub(crate) voting: VotingConfig,
    /// Single-line `[fee_default]` override of `voting.reference_fee`.
    /// Applied by the `voting()` getter.
    pub(crate) fee_default: Option<u64>,

    // ---- validator identity ----
    pub(crate) validation_seed: Option<String>,
    pub(crate) validator_token: Option<String>,
    pub(crate) validator_key_revocation: Option<String>,

    // ---- bare-line lists ----
    pub(crate) ips: Vec<HostPort>,
    pub(crate) ips_fixed: Vec<HostPort>,
    pub(crate) sntp_servers: Vec<String>,
    pub(crate) cluster_nodes: Vec<ClusterNode>,
    pub(crate) trusted_validators: Vec<TrustedValidator>,
    pub(crate) validator_list_sites: Vec<String>,
    pub(crate) validator_list_keys: Vec<String>,
    pub(crate) amendments: Vec<KnownAmendment>,
    pub(crate) veto_amendments: Vec<KnownAmendment>,
    pub(crate) features: HashSet<FeatureName>,
    pub(crate) rpc_startup: Vec<serde_json::Value>,

    // ---- sub-structs ----
    pub(crate) server: ServerConfig,
    /// Keyed by port name. Use `server.port_names` for source order.
    pub(crate) ports: BTreeMap<String, PortConfig>,
    pub(crate) node_db: NodeDbConfig,
    pub(crate) import_db: Option<NodeDbConfig>,
    pub(crate) sqlite: SqliteConfig,
    pub(crate) overlay: OverlayConfig,
    pub(crate) reduce_relay: ReduceRelayConfig,
    pub(crate) crawl: CrawlConfig,
    pub(crate) vl: VlConfig,
    pub(crate) transaction_queue: TxQConfig,
    pub(crate) insight: InsightConfig,
    pub(crate) perf: PerfConfig,
    pub(crate) ledger_tx_tables: LedgerTxTablesConfig,
}

impl Default for Parsed {
    fn default() -> Self {
        Parsed {
            source_format: crate::error::Format::Ini,
            network_id: 0,
            network_quorum: 1,
            peer_private: false,
            peers_max: 0,
            peers_in_max: 0,
            peers_out_max: 0,
            relay_untrusted_validations: RelayPolicy::All,
            relay_untrusted_proposals: RelayPolicy::Trusted,
            node_size: None,
            signing_enabled: false,
            elb_support: false,
            ssl_verify: true,
            ssl_verify_file: None,
            ssl_verify_dir: None,
            ledger_history: LedgerHistory::default(),
            fetch_depth: FetchDepth::default(),
            path_search_old: 2,
            path_search: 2,
            path_search_fast: 2,
            path_search_max: 3,
            max_transactions: 250,
            amendment_majority_time: Duration::from_secs(15 * 60),
            workers: 0,
            io_workers: 0,
            prefetch_workers: 0,
            sweep_interval: None,
            compression: false,
            ledger_replay: false,
            beta_rpc_api: false,
            server_domain: None,
            validator_list_threshold: None,
            websocket_ping_frequency: None,
            debug_logfile: None,
            database_path: None,
            validators_file: None,
            voting: VotingConfig::default(),
            fee_default: None,
            validation_seed: None,
            validator_token: None,
            validator_key_revocation: None,
            ips: Vec::new(),
            ips_fixed: Vec::new(),
            sntp_servers: Vec::new(),
            cluster_nodes: Vec::new(),
            trusted_validators: Vec::new(),
            validator_list_sites: Vec::new(),
            validator_list_keys: Vec::new(),
            amendments: Vec::new(),
            veto_amendments: Vec::new(),
            features: HashSet::new(),
            rpc_startup: Vec::new(),
            server: ServerConfig::default(),
            ports: BTreeMap::new(),
            node_db: NodeDbConfig::default(),
            import_db: None,
            sqlite: SqliteConfig::default(),
            overlay: OverlayConfig::default(),
            reduce_relay: ReduceRelayConfig::default(),
            crawl: CrawlConfig::default(),
            vl: VlConfig::default(),
            transaction_queue: TxQConfig::default(),
            insight: InsightConfig::default(),
            perf: PerfConfig::default(),
            ledger_tx_tables: LedgerTxTablesConfig::default(),
        }
    }
}

/// CLI / programmatic overrides that take precedence over parsed file values.
/// Every field is `Option<T>`; `None` means "use the parsed value".
#[derive(Debug)]
pub(crate) struct Overrides {
    pub(crate) quiet: Option<bool>,
    pub(crate) silent: Option<bool>,
    pub(crate) standalone: Option<bool>,
    pub(crate) start_up: Option<StartUpType>,
    pub(crate) start_ledger: Option<String>,
    pub(crate) start_valid: Option<bool>,
    pub(crate) trap_tx_hash: Option<[u8; 32]>,
    pub(crate) do_import: Option<bool>,
    pub(crate) forced_ledger_range: Option<(u32, u32)>,
    pub(crate) validation_quorum: Option<u64>,
    pub(crate) rpc_ip: Option<SocketAddr>,
    /// Test-only knob: force multi-threaded job queue even in standalone mode.
    /// Never set from a config file. See analysis §2.5 / §7 #15.
    pub(crate) force_multi_thread: Option<bool>,
    /// Set explicitly when using `from_ini_str` / `from_toml_str` so that
    /// bootstrap() knows the config directory. See design §15 Q7.
    pub(crate) config_dir: Option<PathBuf>,
    /// The file path from which config was loaded. Set by `from_file`.
    /// Used by bootstrap() to emit the stderr echo (unless quiet).
    pub(crate) _explicit_config_path: Option<PathBuf>,
}

impl Default for Overrides {
    fn default() -> Self {
        Overrides {
            quiet: None,
            silent: None,
            standalone: None,
            start_up: None,
            start_ledger: None,
            start_valid: None,
            trap_tx_hash: None,
            do_import: None,
            forced_ledger_range: None,
            validation_quorum: None,
            rpc_ip: None,
            force_multi_thread: None,
            config_dir: None,
            _explicit_config_path: None,
        }
    }
}

/// Values that are only available after `bootstrap()` runs.
#[derive(Debug)]
pub(crate) struct Finalized {
    pub(crate) config_dir: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) debug_logfile_resolved: Option<PathBuf>,
    pub(crate) validators_file_resolved: Option<PathBuf>,
    pub(crate) node_size_effective: NodeSize,
}

// ---------------------------------------------------------------------------
// Public Config type
// ---------------------------------------------------------------------------

/// The single user-facing configuration type.
///
/// Lifecycle:
/// 1. `from_file` / `from_ini_str` / `from_toml_str`  — parse
/// 2. `set_*` methods                                  — CLI overrides
/// 3. `bootstrap()`                                    — path resolution, NodeSize detection, etc.
/// 4. getters                                          — read fully-baked values
#[derive(Debug)]
pub struct Config {
    pub(crate) parsed: Parsed,
    pub(crate) overrides: Overrides,
    pub(crate) finalized: Option<Finalized>,
}

impl Config {
    // ---- internal constructors ----

    /// Build a `Config` from a pre-populated `Parsed` bucket.
    /// Format modules (`ini`, `toml`) use this after filling in `Parsed`.
    #[allow(dead_code)]
    pub(crate) fn new_with_parsed(p: Parsed) -> Self {
        Config {
            parsed: p,
            overrides: Overrides::default(),
            finalized: None,
        }
    }

    // ---- public constructors ----

    /// Load and parse a file. Format chosen by extension (`.toml` → TOML, else INI).
    /// Sets `overrides.config_dir` to `path.parent()` so `bootstrap()` can resolve
    /// relative paths. Does NOT run `bootstrap()` — caller does that.
    ///
    /// Note: any prior `set_config_dir` call is unconditionally overwritten with
    /// `path.parent()`.  This is by design — `from_file` is a fresh constructor,
    /// not an incremental builder.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::io(path.to_owned(), e))?;

        let mut cfg = if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            crate::toml::parse_toml(&text)?
        } else {
            crate::ini::parse_ini(&text)?
        };

        // Record config dir so bootstrap() can resolve relative paths.
        if let Some(parent) = path.parent() {
            cfg.overrides.config_dir = Some(parent.to_owned());
        } else {
            cfg.overrides.config_dir = Some(std::path::PathBuf::from("."));
        }

        // Store the explicit path for the stderr echo in bootstrap.
        cfg.overrides._explicit_config_path = Some(path.to_owned());

        Ok(cfg)
    }

    /// Parse an INI blob. No file discovery, no `validators.txt` splice
    /// (call `bootstrap()` once `set_config_dir` has been called).
    pub fn from_ini_str(text: &str) -> Result<Self, ConfigError> {
        crate::ini::parse_ini(text)
    }

    /// Parse a TOML blob. Same caveats as `from_ini_str`.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        crate::toml::parse_toml(text)
    }

    // ---- override setters ----

    pub fn set_quiet(&mut self, v: bool) {
        self.overrides.quiet = Some(v);
    }

    /// Set the silent flag.
    ///
    /// `set_silent(true)` also sets the quiet flag (silent implies quiet).
    /// `set_silent(false)` clears the silent flag but does **not** clear a separately
    /// set quiet flag — the two are independent once the silent→quiet bridge has fired.
    /// Use `set_quiet(false)` explicitly to clear quiet independently.
    pub fn set_silent(&mut self, v: bool) {
        self.overrides.silent = Some(v);
        if v {
            self.overrides.quiet = Some(true); // silent implies quiet
        }
    }

    pub fn set_standalone(&mut self, v: bool) {
        self.overrides.standalone = Some(v);
    }

    pub fn set_start_up(&mut self, v: StartUpType) {
        self.overrides.start_up = Some(v);
    }

    pub fn set_start_ledger(&mut self, v: String) {
        self.overrides.start_ledger = Some(v);
    }

    pub fn set_start_valid(&mut self, v: bool) {
        self.overrides.start_valid = Some(v);
    }

    pub fn set_trap_tx_hash(&mut self, v: [u8; 32]) {
        self.overrides.trap_tx_hash = Some(v);
    }

    pub fn set_do_import(&mut self, v: bool) {
        self.overrides.do_import = Some(v);
    }

    pub fn set_forced_ledger_range(&mut self, lo: u32, hi: u32) {
        self.overrides.forced_ledger_range = Some((lo, hi));
    }

    pub fn set_validation_quorum(&mut self, v: u64) {
        self.overrides.validation_quorum = Some(v);
    }

    pub fn set_rpc_ip(&mut self, v: SocketAddr) {
        self.overrides.rpc_ip = Some(v);
    }

    /// Test-only knob: force multi-threaded job queue even in standalone mode.
    pub fn set_force_multi_thread(&mut self, v: bool) {
        self.overrides.force_multi_thread = Some(v);
    }

    /// Set the config directory explicitly (required when using `from_ini_str` or
    /// `from_toml_str` and then calling `bootstrap()`). See design §15 Q7.
    pub fn set_config_dir(&mut self, p: PathBuf) {
        self.overrides.config_dir = Some(p);
    }

    // ---- finalize ----

    /// Run bootstrap side-effects: resolve paths, detect `NodeSize`, create data
    /// directory (unless standalone), echo config path to stderr (unless quiet),
    /// splice `validators.txt`.
    ///
    /// Idempotent — subsequent calls are no-ops once finalized.
    /// Must be called before any getter that accesses resolved-path or `NodeSize`-derived values.
    pub fn bootstrap(&mut self) -> Result<(), ConfigError> {
        if self.finalized.is_some() {
            return Ok(());
        }
        crate::bootstrap::run_bootstrap(self)
    }

    // ---- helpers ----

    fn require_finalized(&self) -> &Finalized {
        self.finalized.as_ref().unwrap_or_else(|| {
            panic!("Config::bootstrap() must be called before accessing path/NodeSize-derived values")
        })
    }

    // ---- getters: top-level scalars ----

    pub fn network_id(&self) -> u32 {
        self.parsed.network_id
    }

    /// Returns the network_quorum value from the config file.
    /// This is the file-only value (`[network_quorum]` section).
    /// The CLI `--quorum` override is accessible via `validation_quorum()`.
    pub fn network_quorum(&self) -> u64 {
        self.parsed.network_quorum
    }

    pub fn peer_private(&self) -> bool {
        self.parsed.peer_private
    }

    /// Returns the effective ledger history.
    /// Forced to `None_` in standalone mode (design §9).
    pub fn ledger_history(&self) -> LedgerHistory {
        if self.standalone() {
            LedgerHistory::None_
        } else {
            self.parsed.ledger_history
        }
    }

    pub fn fetch_depth(&self) -> FetchDepth {
        self.parsed.fetch_depth
    }

    pub fn max_transactions(&self) -> i32 {
        self.parsed.max_transactions
    }

    pub fn peers_max(&self) -> u32 {
        self.parsed.peers_max
    }

    pub fn peers_in_max(&self) -> u32 {
        self.parsed.peers_in_max
    }

    pub fn peers_out_max(&self) -> u32 {
        self.parsed.peers_out_max
    }

    pub fn signing_enabled(&self) -> bool {
        self.parsed.signing_enabled
    }

    pub fn elb_support(&self) -> bool {
        self.parsed.elb_support
    }

    pub fn ssl_verify(&self) -> bool {
        self.parsed.ssl_verify
    }

    pub fn ssl_verify_file(&self) -> Option<&std::path::Path> {
        self.parsed.ssl_verify_file.as_deref()
    }

    pub fn ssl_verify_dir(&self) -> Option<&std::path::Path> {
        self.parsed.ssl_verify_dir.as_deref()
    }

    pub fn path_search_old(&self) -> i32 {
        self.parsed.path_search_old
    }

    pub fn path_search(&self) -> i32 {
        self.parsed.path_search
    }

    pub fn path_search_fast(&self) -> i32 {
        self.parsed.path_search_fast
    }

    /// Returns the effective `path_search_max`.
    /// Forced to `0` when a validator identity is set (design §9).
    pub fn path_search_max(&self) -> i32 {
        if self.parsed.validation_seed.is_some() || self.parsed.validator_token.is_some() {
            0
        } else {
            self.parsed.path_search_max
        }
    }

    pub fn compression(&self) -> bool {
        self.parsed.compression
    }

    pub fn ledger_replay(&self) -> bool {
        self.parsed.ledger_replay
    }

    pub fn beta_rpc_api(&self) -> bool {
        self.parsed.beta_rpc_api
    }

    pub fn validator_list_threshold(&self) -> Option<u64> {
        self.parsed.validator_list_threshold
    }

    pub fn websocket_ping_frequency(&self) -> Option<u32> {
        self.parsed.websocket_ping_frequency
    }

    pub fn validation_seed(&self) -> Option<&str> {
        self.parsed.validation_seed.as_deref()
    }

    pub fn validator_token(&self) -> Option<&str> {
        self.parsed.validator_token.as_deref()
    }

    pub fn validator_key_revocation(&self) -> Option<&str> {
        self.parsed.validator_key_revocation.as_deref()
    }

    pub fn relay_untrusted_validations(&self) -> RelayPolicy {
        self.parsed.relay_untrusted_validations
    }

    pub fn relay_untrusted_proposals(&self) -> RelayPolicy {
        self.parsed.relay_untrusted_proposals
    }

    pub fn rpc_startup(&self) -> &[serde_json::Value] {
        &self.parsed.rpc_startup
    }

    pub fn validator_list_sites(&self) -> &[String] {
        &self.parsed.validator_list_sites
    }

    pub fn validator_list_keys(&self) -> &[String] {
        &self.parsed.validator_list_keys
    }

    pub fn amendment_majority_time(&self) -> Duration {
        self.parsed.amendment_majority_time
    }

    pub fn workers(&self) -> u32 {
        self.parsed.workers
    }

    pub fn io_workers(&self) -> u32 {
        self.parsed.io_workers
    }

    pub fn prefetch_workers(&self) -> u32 {
        self.parsed.prefetch_workers
    }

    pub fn sweep_interval(&self) -> Option<u32> {
        self.parsed.sweep_interval
    }

    pub fn server_domain(&self) -> Option<&str> {
        self.parsed.server_domain.as_deref()
    }

    /// Returns the set of enabled feature names from `[features]`.
    ///
    /// **Invariant:** feature names are raw strings; validation against the registered
    /// feature list is a Phase 3 concern handled by the downstream C++ consumer.
    /// Unknown feature names *do* survive parse — callers are responsible for rejecting them.
    pub fn features(&self) -> &HashSet<FeatureName> {
        &self.parsed.features
    }

    // ---- getters: post-bootstrap paths ----

    pub fn config_dir(&self) -> &Path {
        &self.require_finalized().config_dir
    }

    pub fn data_dir(&self) -> &Path {
        &self.require_finalized().data_dir
    }

    pub fn debug_logfile(&self) -> Option<&Path> {
        self.require_finalized()
            .debug_logfile_resolved
            .as_deref()
    }

    pub fn validators_file(&self) -> Option<&Path> {
        self.require_finalized()
            .validators_file_resolved
            .as_deref()
    }

    pub fn node_size(&self) -> NodeSize {
        self.require_finalized().node_size_effective
    }

    // ---- getters: CLI-overridable ----

    pub fn start_up(&self) -> StartUpType {
        self.overrides
            .start_up
            .unwrap_or(StartUpType::Normal)
    }

    pub fn start_ledger(&self) -> Option<&str> {
        self.overrides.start_ledger.as_deref()
    }

    pub fn start_valid(&self) -> bool {
        self.overrides.start_valid.unwrap_or(false)
    }

    pub fn do_import(&self) -> bool {
        self.overrides.do_import.unwrap_or(false)
    }

    pub fn forced_ledger_range(&self) -> Option<(u32, u32)> {
        self.overrides.forced_ledger_range
    }

    /// Returns the CLI `--quorum` override if set, otherwise `None`.
    /// This is distinct from `network_quorum()` which returns the file-configured value.
    /// Callers that need the effective quorum should use this override when `Some`, and
    /// fall back to `network_quorum()` when `None`.
    pub fn validation_quorum(&self) -> Option<u64> {
        self.overrides.validation_quorum
    }

    pub fn rpc_ip(&self) -> Option<std::net::SocketAddr> {
        self.overrides.rpc_ip
    }

    pub fn force_multi_thread(&self) -> bool {
        self.overrides.force_multi_thread.unwrap_or(false)
    }

    /// Returns true if quiet mode is active.
    /// Silent mode implies quiet: `silent() || quiet_override`.
    pub fn quiet(&self) -> bool {
        self.silent() || self.overrides.quiet.unwrap_or(false)
    }

    pub fn silent(&self) -> bool {
        self.overrides.silent.unwrap_or(false)
    }

    pub fn standalone(&self) -> bool {
        self.overrides.standalone.unwrap_or(false)
    }

    // ---- getters: sub-structs (by reference) ----

    pub fn server(&self) -> &ServerConfig {
        &self.parsed.server
    }

    pub fn port(&self, name: &str) -> Option<&PortConfig> {
        self.parsed.ports.get(name)
    }

    pub fn ports(&self) -> impl Iterator<Item = &PortConfig> {
        self.parsed.ports.values()
    }

    pub fn node_db(&self) -> &NodeDbConfig {
        &self.parsed.node_db
    }

    pub fn import_db(&self) -> Option<&NodeDbConfig> {
        self.parsed.import_db.as_ref()
    }

    pub fn sqlite(&self) -> &SqliteConfig {
        &self.parsed.sqlite
    }

    pub fn overlay(&self) -> &OverlayConfig {
        &self.parsed.overlay
    }

    pub fn reduce_relay(&self) -> &ReduceRelayConfig {
        &self.parsed.reduce_relay
    }

    pub fn crawl(&self) -> &CrawlConfig {
        &self.parsed.crawl
    }

    /// Returns the merged voting config.
    /// `[fee_default]` overrides `voting.reference_fee` (design §9).
    ///
    /// Returns by value because the merge of two distinct parsed fields
    /// (`voting` + `fee_default`) requires a temporary copy.  The cost is negligible
    /// for a startup-time read.  A future optimisation may cache the merged value in
    /// `Finalized` and return `&VotingConfig` instead.
    pub fn voting(&self) -> VotingConfig {
        let mut v = self.parsed.voting.clone();
        if let Some(f) = self.parsed.fee_default {
            v.reference_fee = f;
        }
        v
    }

    pub fn transaction_queue(&self) -> &TxQConfig {
        &self.parsed.transaction_queue
    }

    pub fn insight(&self) -> &InsightConfig {
        &self.parsed.insight
    }

    pub fn perf(&self) -> &PerfConfig {
        &self.parsed.perf
    }

    pub fn ledger_tx_tables(&self) -> &LedgerTxTablesConfig {
        &self.parsed.ledger_tx_tables
    }

    pub fn vl(&self) -> &VlConfig {
        &self.parsed.vl
    }

    // ---- getters: bare-line lists ----

    pub fn ips(&self) -> &[HostPort] {
        &self.parsed.ips
    }

    pub fn ips_fixed(&self) -> &[HostPort] {
        &self.parsed.ips_fixed
    }

    pub fn sntp_servers(&self) -> &[String] {
        &self.parsed.sntp_servers
    }

    pub fn cluster_nodes(&self) -> &[ClusterNode] {
        &self.parsed.cluster_nodes
    }

    pub fn amendments(&self) -> &[KnownAmendment] {
        &self.parsed.amendments
    }

    pub fn veto_amendments(&self) -> &[KnownAmendment] {
        &self.parsed.veto_amendments
    }

    pub fn trusted_validators(&self) -> &[TrustedValidator] {
        &self.parsed.trusted_validators
    }

    // ---- sized-item table lookup ----

    /// Look up a sized value using the effective `NodeSize` (requires bootstrap).
    pub fn sized_value(&self, item: SizedItem) -> i32 {
        let size = self.node_size();
        crate::types::sized::sized_value(item, size)
    }

    /// Look up a sized value for an explicit `NodeSize`.
    pub fn sized_value_for(&self, item: SizedItem, node: NodeSize) -> i32 {
        crate::types::sized::sized_value(item, node)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_smoke_test() {
        let text = "[overlay]\nmax_unknown_time=600\n";
        let mut cfg = Config::from_ini_str(text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true); // skip mkdir
        cfg.bootstrap().unwrap();
        assert_eq!(cfg.overlay().max_unknown_time, 600);
    }

    #[test]
    fn path_search_max_forced_zero_with_validator_token() {
        let text = "[validator_token]\nsome_token\n";
        let cfg = Config::from_ini_str(text).unwrap();
        assert_eq!(cfg.path_search_max(), 0);
    }

    #[test]
    fn path_search_max_default_without_identity() {
        let cfg = Config::from_ini_str("").unwrap();
        assert_eq!(cfg.path_search_max(), 3); // default
    }

    #[test]
    fn ledger_history_forced_none_in_standalone() {
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_standalone(true);
        assert_eq!(cfg.ledger_history(), LedgerHistory::None_);
    }

    #[test]
    fn bootstrap_idempotent() {
        let text = "";
        let mut cfg = Config::from_ini_str(text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        cfg.bootstrap().unwrap(); // second call should be no-op
        assert!(cfg.finalized.is_some());
    }

    #[test]
    fn from_file_nonexistent_returns_error() {
        let result = Config::from_file(std::path::Path::new("/nonexistent/path/config.cfg"));
        assert!(result.is_err());
    }

    #[test]
    fn bootstrap_requires_config_dir() {
        let mut cfg = Config::from_ini_str("").unwrap();
        // No set_config_dir → should error
        let result = cfg.bootstrap();
        assert!(result.is_err());
    }

    // ---- full lifecycle smoke tests ----

    #[test]
    fn toml_full_lifecycle_standalone() {
        let toml_text = r#"
            network_id = 99
            compression = true

            [server]
            ports = ["rpc_admin"]

            [port.rpc_admin]
            port = 5005
            protocol = ["Http"]
        "#;
        let mut cfg = Config::from_toml_str(toml_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();

        assert_eq!(cfg.network_id(), 99);
        assert!(cfg.compression());
        assert_eq!(cfg.node_size(), crate::NodeSize::Tiny); // standalone forces Tiny
        assert_eq!(cfg.port("rpc_admin").unwrap().port, 5005);
    }

    #[test]
    fn ini_full_lifecycle_standalone() {
        let ini_text = "[network_id]\n42\n[overlay]\nmax_unknown_time=600\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();

        assert_eq!(cfg.network_id(), 42);
        assert_eq!(cfg.overlay().max_unknown_time, 600);
        assert_eq!(cfg.node_size(), crate::NodeSize::Tiny);
    }

    #[test]
    fn config_set_overrides_before_bootstrap() {
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.set_quiet(true);
        cfg.set_start_up(crate::StartUpType::Normal);
        cfg.bootstrap().unwrap();

        assert!(cfg.quiet());
        assert!(cfg.standalone());
    }

    #[test]
    fn config_data_dir_resolved_after_bootstrap() {
        let mut cfg = Config::from_ini_str("").unwrap();
        let config_dir = std::env::temp_dir();
        cfg.set_config_dir(config_dir.clone());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();

        // data_dir = config_dir/db when database_path not set
        let expected = config_dir.join("db");
        assert_eq!(cfg.data_dir(), expected.as_path());
        assert_eq!(cfg.config_dir(), config_dir.as_path());
    }

    #[test]
    fn config_ledger_history_forced_none_standalone() {
        let ini_text = "[ledger_history]\n1000\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        // standalone forces LedgerHistory::None_
        assert_eq!(cfg.ledger_history(), crate::LedgerHistory::None_);
    }

    #[test]
    fn config_validation_quorum_override() {
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_validation_quorum(7);
        // validation_quorum returns the CLI override as Option<u64>
        assert_eq!(cfg.validation_quorum(), Some(7));
        // network_quorum returns the file value (default 1, not the CLI override)
        assert_eq!(cfg.network_quorum(), 1);
    }

    #[test]
    fn config_path_search_max_zero_with_validation_seed() {
        let ini_text = "[validation_seed]\nmy_seed\n";
        let cfg = Config::from_ini_str(ini_text).unwrap();
        // path_search_max is forced to 0 when validation_seed is set
        assert_eq!(cfg.path_search_max(), 0);
    }

    #[test]
    fn config_ssl_verify_from_toml() {
        let mut cfg = Config::from_toml_str(r#"ssl_verify = false"#).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert!(!cfg.ssl_verify());
    }

    #[test]
    fn config_bootstrap_idempotent_toml() {
        let mut cfg = Config::from_toml_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        cfg.bootstrap().unwrap(); // second call is no-op
        assert!(cfg.finalized.is_some());
    }

    #[test]
    fn config_from_toml_str_features() {
        let mut cfg = Config::from_toml_str(r#"features = ["Flow"]"#).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert!(cfg.features().contains("Flow"));
    }
}

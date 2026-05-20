//! TOML schema types and conversion to `Config`.
//!
//! `Root` mirrors `Parsed` 1:1 with TOML-idiomatic shapes.
//! `#[serde(deny_unknown_fields)]` on `Root` (and wrappers below) enforces
//! strict-mode: unknown keys are errors.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::config::{Config, Parsed};
use crate::error::ConfigError;
use crate::types::*;

// ---------------------------------------------------------------------------
// TOML-specific wrapper for CrawlConfig
// ---------------------------------------------------------------------------
// The types/crawl.rs CrawlConfig has a LegacyBool variant (INI-only).
// In TOML mode we only accept the Detailed shape. We use a local struct and
// convert it.

/// TOML-only `[crawl]` section — accepts only the detailed kv form.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlCrawlConfig {
    pub overlay: bool,
    pub server: bool,
    pub counts: bool,
    pub unl: bool,
}

impl From<TomlCrawlConfig> for CrawlConfig {
    fn from(t: TomlCrawlConfig) -> Self {
        CrawlConfig::Detailed {
            overlay: t.overlay,
            server: t.server,
            counts: t.counts,
            unl: t.unl,
        }
    }
}

// ---------------------------------------------------------------------------
// TOML-specific ServerConfig wrapper
// ---------------------------------------------------------------------------
// The shared ServerConfig uses port_names / defaults. In TOML the field is
// named `ports` (an array of strings). We wrap here and convert.

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlServerConfig {
    /// Names of port sub-tables in `[port.<name>]`. TOML name: `ports`.
    pub ports: Vec<String>,
    /// Server-level defaults — same fields as PortDefaults, flattened.
    #[serde(flatten)]
    pub defaults: TomlPortDefaults,
}

/// A serde-friendly PortDefaults that uses deny_unknown_fields.
/// We need a separate struct because the shared PortDefaults doesn't have
/// deny_unknown_fields (INI is lenient).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlPortDefaults {
    pub ip: Option<std::net::IpAddr>,
    pub protocol: Vec<PortProtocol>,
    pub admin: Vec<ipnet::IpNet>,
    pub secure_gateway: Vec<ipnet::IpNet>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub limit: PortLimit,
    pub send_queue_limit: u16,
    pub ssl_key: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
    pub ssl_chain: Option<PathBuf>,
    pub ssl_ciphers: Option<String>,
    pub ssl_cert_chain: Option<PathBuf>,
    pub ssl_client_ca: Option<PathBuf>,
    pub permessage_deflate: bool,
    pub client_max_window_bits: u8,
    pub server_max_window_bits: u8,
    pub client_no_context_takeover: bool,
    pub server_no_context_takeover: bool,
    pub compress_level: u8,
    pub memory_level: u8,
}

impl Default for TomlPortDefaults {
    fn default() -> Self {
        let d = PortDefaults::default();
        TomlPortDefaults {
            ip: d.ip,
            protocol: d.protocol,
            admin: d.admin,
            secure_gateway: d.secure_gateway,
            user: d.user,
            password: d.password,
            admin_user: d.admin_user,
            admin_password: d.admin_password,
            limit: d.limit,
            send_queue_limit: d.send_queue_limit,
            ssl_key: d.ssl_key,
            ssl_cert: d.ssl_cert,
            ssl_chain: d.ssl_chain,
            ssl_ciphers: d.ssl_ciphers,
            ssl_cert_chain: d.ssl_cert_chain,
            ssl_client_ca: d.ssl_client_ca,
            permessage_deflate: d.permessage_deflate,
            client_max_window_bits: d.client_max_window_bits,
            server_max_window_bits: d.server_max_window_bits,
            client_no_context_takeover: d.client_no_context_takeover,
            server_no_context_takeover: d.server_no_context_takeover,
            compress_level: d.compress_level,
            memory_level: d.memory_level,
        }
    }
}

impl From<TomlPortDefaults> for PortDefaults {
    fn from(t: TomlPortDefaults) -> Self {
        PortDefaults {
            ip: t.ip,
            protocol: t.protocol,
            admin: t.admin,
            secure_gateway: t.secure_gateway,
            user: t.user,
            password: t.password,
            admin_user: t.admin_user,
            admin_password: t.admin_password,
            limit: t.limit,
            send_queue_limit: t.send_queue_limit,
            ssl_key: t.ssl_key,
            ssl_cert: t.ssl_cert,
            ssl_chain: t.ssl_chain,
            ssl_ciphers: t.ssl_ciphers,
            ssl_cert_chain: t.ssl_cert_chain,
            ssl_client_ca: t.ssl_client_ca,
            permessage_deflate: t.permessage_deflate,
            client_max_window_bits: t.client_max_window_bits,
            server_max_window_bits: t.server_max_window_bits,
            client_no_context_takeover: t.client_no_context_takeover,
            server_no_context_takeover: t.server_no_context_takeover,
            compress_level: t.compress_level,
            memory_level: t.memory_level,
        }
    }
}

// ---------------------------------------------------------------------------
// TOML-specific PortConfig wrapper (per-port table)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlPortConfig {
    pub port: u16,
    #[serde(default)]
    pub ip: Option<std::net::IpAddr>,
    #[serde(default)]
    pub protocol: Vec<PortProtocol>,
    #[serde(default)]
    pub admin: Vec<ipnet::IpNet>,
    #[serde(default)]
    pub secure_gateway: Vec<ipnet::IpNet>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub admin_user: Option<String>,
    #[serde(default)]
    pub admin_password: Option<String>,
    #[serde(default)]
    pub limit: PortLimit,
    #[serde(default = "default_send_queue_limit")]
    pub send_queue_limit: u16,
    #[serde(default)]
    pub ssl_key: Option<PathBuf>,
    #[serde(default)]
    pub ssl_cert: Option<PathBuf>,
    #[serde(default)]
    pub ssl_chain: Option<PathBuf>,
    #[serde(default)]
    pub ssl_ciphers: Option<String>,
    #[serde(default)]
    pub ssl_cert_chain: Option<PathBuf>,
    #[serde(default)]
    pub ssl_client_ca: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub permessage_deflate: bool,
    #[serde(default = "default_window_bits")]
    pub client_max_window_bits: u8,
    #[serde(default = "default_window_bits")]
    pub server_max_window_bits: u8,
    #[serde(default)]
    pub client_no_context_takeover: bool,
    #[serde(default)]
    pub server_no_context_takeover: bool,
    #[serde(default = "default_compress_level")]
    pub compress_level: u8,
    #[serde(default = "default_memory_level")]
    pub memory_level: u8,
}

fn default_send_queue_limit() -> u16 { 100 }
fn default_true() -> bool { true }
fn default_window_bits() -> u8 { 15 }
fn default_compress_level() -> u8 { 8 }
fn default_memory_level() -> u8 { 4 }

// ---------------------------------------------------------------------------
// TOML-specific NodeDbConfig wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlNodeDbConfig {
    pub kind: NodeDbKind,
    pub path: PathBuf,
    pub fast_load: bool,
    pub earliest_seq: u32,
    pub online_delete: Option<u32>,
    pub advisory_delete: bool,
    pub delete_batch: u32,
    pub back_off_milliseconds: u32,
    pub age_threshold_seconds: u32,
    pub recovery_wait_seconds: u32,
    pub nudb_block_size: u32,
    /// Extra RocksDB tunables under `[node_db.extras]`.
    #[serde(default)]
    pub extras: BTreeMap<String, String>,
}

impl Default for TomlNodeDbConfig {
    fn default() -> Self {
        let d = NodeDbConfig::default();
        TomlNodeDbConfig {
            kind: d.kind,
            path: d.path,
            fast_load: d.fast_load,
            earliest_seq: d.earliest_seq,
            online_delete: d.online_delete,
            advisory_delete: d.advisory_delete,
            delete_batch: d.delete_batch,
            back_off_milliseconds: d.back_off_milliseconds,
            age_threshold_seconds: d.age_threshold_seconds,
            recovery_wait_seconds: d.recovery_wait_seconds,
            nudb_block_size: d.nudb_block_size,
            extras: BTreeMap::new(),
        }
    }
}

impl From<TomlNodeDbConfig> for NodeDbConfig {
    fn from(t: TomlNodeDbConfig) -> Self {
        NodeDbConfig {
            kind: t.kind,
            path: t.path,
            fast_load: t.fast_load,
            earliest_seq: t.earliest_seq,
            online_delete: t.online_delete,
            advisory_delete: t.advisory_delete,
            delete_batch: t.delete_batch,
            back_off_milliseconds: t.back_off_milliseconds,
            age_threshold_seconds: t.age_threshold_seconds,
            recovery_wait_seconds: t.recovery_wait_seconds,
            nudb_block_size: t.nudb_block_size,
            backend_extras: t.extras,
        }
    }
}

// ---------------------------------------------------------------------------
// TOML-specific SqliteConfig wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlSqliteConfig {
    pub safety_level: Option<SqliteSafety>,
    pub journal_mode: Option<SqliteJournalMode>,
    pub synchronous: Option<SqliteSynchronous>,
    pub temp_store: Option<SqliteTempStore>,
    pub page_size: Option<u32>,
    pub journal_size_limit: Option<i64>,
}

// ---------------------------------------------------------------------------
// TOML-specific OverlayConfig wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlOverlayConfig {
    pub public_ip: Option<std::net::IpAddr>,
    pub ip_limit: Option<u32>,
    pub max_unknown_time: Option<u32>,
    pub max_diverged_time: Option<u32>,
}

// ---------------------------------------------------------------------------
// TOML-specific ReduceRelayConfig wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlReduceRelayConfig {
    pub vp_base_squelch_enable: Option<bool>,
    pub vp_base_squelch_max_selected_peers: Option<u32>,
    pub tx_enable: Option<bool>,
    pub tx_metrics: Option<bool>,
    pub tx_min_peers: Option<u32>,
    pub tx_relay_percentage: Option<u32>,
}

// ---------------------------------------------------------------------------
// TOML-specific TxQConfig wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlTxQConfig {
    pub ledgers_in_queue: Option<u32>,
    pub minimum_queue_size: Option<u32>,
    pub retry_sequence_percent: Option<u32>,
    pub minimum_escalation_multiplier: Option<u32>,
    pub minimum_txn_in_ledger: Option<u32>,
    pub minimum_txn_in_ledger_standalone: Option<u32>,
    pub target_txn_in_ledger: Option<u32>,
    pub maximum_txn_in_ledger: Option<u32>,
    pub normal_consensus_increase_percent: Option<u32>,
    pub slow_consensus_decrease_percent: Option<u32>,
    pub maximum_txn_per_account: Option<u32>,
    pub minimum_last_ledger_buffer: Option<u32>,
    pub zero_basefee_transaction_feelevel: Option<u32>,
}

// ---------------------------------------------------------------------------
// Root — top-level TOML document
// ---------------------------------------------------------------------------

/// Top-level TOML document structure. Mirrors `Parsed` 1:1 in field names.
/// `deny_unknown_fields` makes any unrecognised top-level key an error.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Root {
    // ---- top-level scalars ----
    pub network_id: u32,
    pub network_quorum: u64,
    pub peer_private: bool,
    pub peers_max: u32,
    pub peers_in_max: u32,
    pub peers_out_max: u32,
    pub relay_untrusted_validations: Option<RelayPolicy>,
    pub relay_untrusted_proposals: Option<RelayPolicy>,
    pub node_size: Option<NodeSize>,
    pub signing_enabled: bool,
    pub elb_support: bool,
    pub ssl_verify: Option<bool>,
    pub ssl_verify_file: Option<PathBuf>,
    pub ssl_verify_dir: Option<PathBuf>,
    pub ledger_history: Option<LedgerHistory>,
    pub fetch_depth: Option<FetchDepth>,
    pub path_search_old: Option<i32>,
    pub path_search: Option<i32>,
    pub path_search_fast: Option<i32>,
    pub path_search_max: Option<i32>,
    pub max_transactions: Option<i32>,
    pub amendment_majority_time: Option<String>,
    pub workers: Option<u32>,
    pub io_workers: Option<u32>,
    pub prefetch_workers: Option<u32>,
    pub sweep_interval: Option<u32>,
    pub compression: bool,
    pub ledger_replay: bool,
    pub beta_rpc_api: bool,
    pub server_domain: Option<String>,
    pub validator_list_threshold: Option<u64>,
    pub websocket_ping_frequency: Option<u32>,

    // ---- path fields ----
    pub debug_logfile: Option<PathBuf>,
    pub database_path: Option<PathBuf>,
    pub validators_file: Option<PathBuf>,

    // ---- fee / voting ----
    pub voting: Option<TomlVotingConfig>,
    pub fee_default: Option<u64>,

    // ---- validator identity ----
    pub validation_seed: Option<String>,
    pub validator_token: Option<String>,
    pub validator_key_revocation: Option<String>,

    // ---- bare-line lists ----
    pub ips: Vec<HostPort>,
    pub ips_fixed: Vec<HostPort>,
    pub sntp_servers: Vec<String>,
    pub cluster_nodes: Vec<ClusterNode>,
    pub validators: Vec<TrustedValidator>,
    pub validator_list_sites: Vec<String>,
    pub validator_list_keys: Vec<String>,
    pub amendments: Vec<KnownAmendment>,
    pub veto_amendments: Vec<KnownAmendment>,
    pub features: HashSet<FeatureName>,
    pub rpc_startup: Vec<serde_json::Value>,

    // ---- sub-structs / tables ----
    pub server: TomlServerConfig,
    /// `[port.<name>]` — table of tables.
    pub port: BTreeMap<String, TomlPortConfig>,
    pub node_db: TomlNodeDbConfig,
    pub import_db: Option<TomlNodeDbConfig>,
    pub sqlite: TomlSqliteConfig,
    pub overlay: TomlOverlayConfig,
    pub reduce_relay: TomlReduceRelayConfig,
    pub crawl: TomlCrawlConfig,
    pub vl: Option<TomlVlConfig>,
    pub transaction_queue: TomlTxQConfig,
    pub insight: Option<TomlInsightConfig>,
    pub perf: Option<TomlPerfConfig>,
    pub ledger_tx_tables: Option<TomlLedgerTxTablesConfig>,
}

// ---------------------------------------------------------------------------
// Thin TOML wrappers for simple structs that just need deny_unknown_fields
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlVotingConfig {
    pub reference_fee: Option<u64>,
    pub account_reserve: Option<u64>,
    pub owner_reserve: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlVlConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlInsightConfig {
    pub server: Option<InsightServer>,
    pub address: Option<std::net::SocketAddr>,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlPerfConfig {
    pub perf_log: Option<PathBuf>,
    pub log_interval: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub(super) struct TomlLedgerTxTablesConfig {
    pub use_tx_tables: Option<bool>,
}

// ---------------------------------------------------------------------------
// validate_strict() implementations
// ---------------------------------------------------------------------------

impl OverlayConfig {
    pub(crate) fn validate_strict(&self) -> Result<(), ConfigError> {
        if !(300..=1800).contains(&self.max_unknown_time) {
            return Err(ConfigError::out_of_range(
                "overlay.max_unknown_time",
                self.max_unknown_time as i64,
                Some(300),
                Some(1800),
            ));
        }
        if !(60..=900).contains(&self.max_diverged_time) {
            return Err(ConfigError::out_of_range(
                "overlay.max_diverged_time",
                self.max_diverged_time as i64,
                Some(60),
                Some(900),
            ));
        }
        Ok(())
    }
}

impl ReduceRelayConfig {
    pub(crate) fn validate_strict(&self) -> Result<(), ConfigError> {
        if self.vp_base_squelch_max_selected_peers < 3 {
            return Err(ConfigError::out_of_range(
                "reduce_relay.vp_base_squelch_max_selected_peers",
                self.vp_base_squelch_max_selected_peers as i64,
                Some(3),
                None,
            ));
        }
        if self.tx_min_peers < 10 {
            return Err(ConfigError::out_of_range(
                "reduce_relay.tx_min_peers",
                self.tx_min_peers as i64,
                Some(10),
                None,
            ));
        }
        if !(10..=100).contains(&self.tx_relay_percentage) {
            return Err(ConfigError::out_of_range(
                "reduce_relay.tx_relay_percentage",
                self.tx_relay_percentage as i64,
                Some(10),
                Some(100),
            ));
        }
        Ok(())
    }
}

impl NodeDbConfig {
    pub(crate) fn validate_strict(&self, section: &str) -> Result<(), ConfigError> {
        if self.earliest_seq < 1 {
            return Err(ConfigError::out_of_range(
                &format!("{section}.earliest_seq"),
                self.earliest_seq as i64,
                Some(1),
                None,
            ));
        }
        // nudb_block_size must be power-of-2 in 4096..=32768
        let bs = self.nudb_block_size;
        if bs < 4096 || bs > 32768 || !bs.is_power_of_two() {
            return Err(ConfigError::out_of_range(
                &format!("{section}.nudb_block_size"),
                bs as i64,
                Some(4096),
                Some(32768),
            ));
        }
        if let Some(od) = self.online_delete {
            if od < 256 {
                return Err(ConfigError::out_of_range(
                    &format!("{section}.online_delete"),
                    od as i64,
                    Some(256),
                    None,
                ));
            }
        }
        Ok(())
    }
}

impl SqliteConfig {
    pub(crate) fn validate_strict(&self) -> Result<(), ConfigError> {
        if let SqliteMode::Tuning { page_size, .. } = &self.mode {
            let ps = *page_size;
            if ps < 512 || ps > 65536 || !ps.is_power_of_two() {
                return Err(ConfigError::out_of_range(
                    "sqlite.page_size",
                    ps as i64,
                    Some(512),
                    Some(65536),
                ));
            }
        }
        Ok(())
    }
}

impl TxQConfig {
    pub(crate) fn validate_strict(&self) -> Result<(), ConfigError> {
        if !(0..=1000).contains(&self.normal_consensus_increase_percent) {
            return Err(ConfigError::out_of_range(
                "transaction_queue.normal_consensus_increase_percent",
                self.normal_consensus_increase_percent as i64,
                Some(0),
                Some(1000),
            ));
        }
        if !(0..=100).contains(&self.slow_consensus_decrease_percent) {
            return Err(ConfigError::out_of_range(
                "transaction_queue.slow_consensus_decrease_percent",
                self.slow_consensus_decrease_percent as i64,
                Some(0),
                Some(100),
            ));
        }
        if let Some(max) = self.maximum_txn_in_ledger {
            if max < self.minimum_txn_in_ledger {
                return Err(ConfigError::cross(format!(
                    "transaction_queue.maximum_txn_in_ledger ({max}) must be >= \
                     minimum_txn_in_ledger ({})",
                    self.minimum_txn_in_ledger
                )));
            }
        }
        Ok(())
    }
}

impl PortDefaults {
    pub(crate) fn validate_strict(&self, ctx: &str) -> Result<(), ConfigError> {
        if self.send_queue_limit == 0 {
            return Err(ConfigError::out_of_range(
                &format!("{ctx}.send_queue_limit"),
                0,
                Some(1),
                None,
            ));
        }
        if !(9..=15).contains(&self.client_max_window_bits) {
            return Err(ConfigError::out_of_range(
                &format!("{ctx}.client_max_window_bits"),
                self.client_max_window_bits as i64,
                Some(9),
                Some(15),
            ));
        }
        if !(9..=15).contains(&self.server_max_window_bits) {
            return Err(ConfigError::out_of_range(
                &format!("{ctx}.server_max_window_bits"),
                self.server_max_window_bits as i64,
                Some(9),
                Some(15),
            ));
        }
        if self.compress_level > 9 {
            return Err(ConfigError::out_of_range(
                &format!("{ctx}.compress_level"),
                self.compress_level as i64,
                Some(0),
                Some(9),
            ));
        }
        if !(1..=9).contains(&self.memory_level) {
            return Err(ConfigError::out_of_range(
                &format!("{ctx}.memory_level"),
                self.memory_level as i64,
                Some(1),
                Some(9),
            ));
        }
        Ok(())
    }
}

impl PortConfig {
    pub(crate) fn validate_strict(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::out_of_range(
                &format!("port.{}.port", self.name),
                0,
                Some(1),
                None,
            ));
        }
        self.effective.validate_strict(&format!("port.{}", self.name))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Top-level Parsed strict validation
// ---------------------------------------------------------------------------

impl Parsed {
    pub(crate) fn validate_strict_toplevel(&self) -> Result<(), ConfigError> {
        // max_transactions in 100..=1000
        if !(100..=1000).contains(&self.max_transactions) {
            return Err(ConfigError::out_of_range(
                "max_transactions",
                self.max_transactions as i64,
                Some(100),
                Some(1000),
            ));
        }
        // fetch_depth >= 10 when it's a Count
        if let FetchDepth::Count(n) = self.fetch_depth {
            if n < 10 {
                return Err(ConfigError::out_of_range(
                    "fetch_depth",
                    n as i64,
                    Some(10),
                    None,
                ));
            }
        }
        // workers in 1..=1024 (0 means "auto", allowed)
        for (name, val) in [
            ("workers", self.workers),
            ("io_workers", self.io_workers),
            ("prefetch_workers", self.prefetch_workers),
        ] {
            if val > 0 && val > 1024 {
                return Err(ConfigError::out_of_range(
                    name,
                    val as i64,
                    Some(1),
                    Some(1024),
                ));
            }
        }
        // sweep_interval in 10..=600 when set
        if let Some(si) = self.sweep_interval {
            if !(10..=600).contains(&si) {
                return Err(ConfigError::out_of_range(
                    "sweep_interval",
                    si as i64,
                    Some(10),
                    Some(600),
                ));
            }
        }
        // validation_seed XOR validator_token
        if self.validation_seed.is_some() && self.validator_token.is_some() {
            return Err(ConfigError::mutual_exclusion(
                "validation_seed",
                "validator_token",
            ));
        }
        // validator_list_threshold <= |validator_list_keys|
        if let Some(thr) = self.validator_list_threshold {
            let n = self.validator_list_keys.len() as u64;
            if thr > n {
                return Err(ConfigError::cross(format!(
                    "validator_list_threshold ({thr}) exceeds validator_list_keys count ({n})"
                )));
            }
        }
        // network_quorum <= peers_max (only when peers_max is non-zero)
        if self.peers_max > 0 && self.network_quorum > self.peers_max as u64 {
            return Err(ConfigError::cross(format!(
                "network_quorum ({}) exceeds peers_max ({})",
                self.network_quorum, self.peers_max
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Conversion: Root → Config
// ---------------------------------------------------------------------------

/// Convert a deserialised `Root` into a fully-populated `Config`.
/// Returns an error if any strict validation fails.
pub(super) fn root_to_config(root: Root) -> Result<Config, ConfigError> {
    let mut parsed = Parsed::default();

    // ---- scalars ----
    parsed.network_id = root.network_id;
    parsed.network_quorum = root.network_quorum;
    parsed.peer_private = root.peer_private;
    parsed.peers_max = root.peers_max;
    parsed.peers_in_max = root.peers_in_max;
    parsed.peers_out_max = root.peers_out_max;
    if let Some(v) = root.relay_untrusted_validations {
        parsed.relay_untrusted_validations = v;
    }
    if let Some(v) = root.relay_untrusted_proposals {
        parsed.relay_untrusted_proposals = v;
    }
    parsed.node_size = root.node_size;
    parsed.signing_enabled = root.signing_enabled;
    parsed.elb_support = root.elb_support;
    if let Some(v) = root.ssl_verify {
        parsed.ssl_verify = v;
    }
    parsed.ssl_verify_file = root.ssl_verify_file;
    parsed.ssl_verify_dir = root.ssl_verify_dir;
    if let Some(v) = root.ledger_history {
        parsed.ledger_history = v;
    }
    if let Some(v) = root.fetch_depth {
        parsed.fetch_depth = v;
    }
    if let Some(v) = root.path_search_old {
        parsed.path_search_old = v;
    }
    if let Some(v) = root.path_search {
        parsed.path_search = v;
    }
    if let Some(v) = root.path_search_fast {
        parsed.path_search_fast = v;
    }
    if let Some(v) = root.path_search_max {
        parsed.path_search_max = v;
    }
    if let Some(v) = root.max_transactions {
        parsed.max_transactions = v;
    }
    if let Some(ref s) = root.amendment_majority_time {
        parsed.amendment_majority_time =
            parse_amendment_majority_time(s).map_err(|e| {
                ConfigError::grammar("amendment_majority_time", s.as_str(), e.to_string())
            })?;
    }
    if let Some(v) = root.workers {
        parsed.workers = v;
    }
    if let Some(v) = root.io_workers {
        parsed.io_workers = v;
    }
    if let Some(v) = root.prefetch_workers {
        parsed.prefetch_workers = v;
    }
    parsed.sweep_interval = root.sweep_interval;
    parsed.compression = root.compression;
    parsed.ledger_replay = root.ledger_replay;
    parsed.beta_rpc_api = root.beta_rpc_api;
    parsed.server_domain = root.server_domain;
    parsed.validator_list_threshold = root.validator_list_threshold;
    parsed.websocket_ping_frequency = root.websocket_ping_frequency;

    // ---- path fields ----
    parsed.debug_logfile = root.debug_logfile.map(RelPath::from);
    parsed.database_path = root.database_path.map(RelPath::from);
    parsed.validators_file = root.validators_file.map(RelPath::from);

    // ---- voting ----
    if let Some(v) = root.voting {
        if let Some(rf) = v.reference_fee {
            parsed.voting.reference_fee = rf;
        }
        if let Some(ar) = v.account_reserve {
            parsed.voting.account_reserve = ar;
        }
        if let Some(or_) = v.owner_reserve {
            parsed.voting.owner_reserve = or_;
        }
    }
    parsed.fee_default = root.fee_default;

    // ---- validator identity ----
    parsed.validation_seed = root.validation_seed;
    parsed.validator_token = root.validator_token;
    parsed.validator_key_revocation = root.validator_key_revocation;

    // ---- bare-line lists ----
    parsed.ips = root.ips;
    parsed.ips_fixed = root.ips_fixed;
    parsed.sntp_servers = root.sntp_servers;
    parsed.cluster_nodes = root.cluster_nodes;
    parsed.trusted_validators = root.validators;
    parsed.validator_list_sites = root.validator_list_sites;
    parsed.validator_list_keys = root.validator_list_keys;
    parsed.amendments = root.amendments;
    parsed.veto_amendments = root.veto_amendments;
    parsed.features = root.features;
    parsed.rpc_startup = root.rpc_startup;

    // ---- overlay ----
    {
        let d = OverlayConfig::default();
        parsed.overlay = OverlayConfig {
            public_ip: root.overlay.public_ip,
            ip_limit: root.overlay.ip_limit,
            max_unknown_time: root.overlay.max_unknown_time.unwrap_or(d.max_unknown_time),
            max_diverged_time: root.overlay.max_diverged_time.unwrap_or(d.max_diverged_time),
        };
        parsed.overlay.validate_strict()?;
    }

    // ---- reduce_relay ----
    {
        let d = ReduceRelayConfig::default();
        parsed.reduce_relay = ReduceRelayConfig {
            vp_base_squelch_enable: root
                .reduce_relay
                .vp_base_squelch_enable
                .unwrap_or(d.vp_base_squelch_enable),
            vp_base_squelch_max_selected_peers: root
                .reduce_relay
                .vp_base_squelch_max_selected_peers
                .unwrap_or(d.vp_base_squelch_max_selected_peers),
            tx_enable: root.reduce_relay.tx_enable.unwrap_or(d.tx_enable),
            tx_metrics: root.reduce_relay.tx_metrics.unwrap_or(d.tx_metrics),
            tx_min_peers: root.reduce_relay.tx_min_peers.unwrap_or(d.tx_min_peers),
            tx_relay_percentage: root
                .reduce_relay
                .tx_relay_percentage
                .unwrap_or(d.tx_relay_percentage),
        };
        parsed.reduce_relay.validate_strict()?;
    }

    // ---- crawl ----
    parsed.crawl = CrawlConfig::from(root.crawl);

    // ---- vl ----
    parsed.vl = root
        .vl
        .map(|v| VlConfig { enabled: v.enabled })
        .unwrap_or_default();

    // ---- node_db ----
    parsed.node_db = NodeDbConfig::from(root.node_db);
    parsed.node_db.validate_strict("node_db")?;

    // ---- import_db ----
    parsed.import_db = root
        .import_db
        .map(|db| -> Result<NodeDbConfig, ConfigError> {
            let c = NodeDbConfig::from(db);
            c.validate_strict("import_db")?;
            Ok(c)
        })
        .transpose()?;

    // ---- sqlite ----
    parsed.sqlite = convert_sqlite(root.sqlite)?;
    parsed.sqlite.validate_strict()?;

    // ---- transaction_queue ----
    {
        let d = TxQConfig::default();
        parsed.transaction_queue = TxQConfig {
            ledgers_in_queue: root
                .transaction_queue
                .ledgers_in_queue
                .unwrap_or(d.ledgers_in_queue),
            minimum_queue_size: root
                .transaction_queue
                .minimum_queue_size
                .unwrap_or(d.minimum_queue_size),
            retry_sequence_percent: root
                .transaction_queue
                .retry_sequence_percent
                .unwrap_or(d.retry_sequence_percent),
            minimum_escalation_multiplier: root
                .transaction_queue
                .minimum_escalation_multiplier
                .unwrap_or(d.minimum_escalation_multiplier),
            minimum_txn_in_ledger: root
                .transaction_queue
                .minimum_txn_in_ledger
                .unwrap_or(d.minimum_txn_in_ledger),
            minimum_txn_in_ledger_standalone: root
                .transaction_queue
                .minimum_txn_in_ledger_standalone
                .unwrap_or(d.minimum_txn_in_ledger_standalone),
            target_txn_in_ledger: root
                .transaction_queue
                .target_txn_in_ledger
                .unwrap_or(d.target_txn_in_ledger),
            maximum_txn_in_ledger: root
                .transaction_queue
                .maximum_txn_in_ledger
                .or(d.maximum_txn_in_ledger),
            normal_consensus_increase_percent: root
                .transaction_queue
                .normal_consensus_increase_percent
                .unwrap_or(d.normal_consensus_increase_percent),
            slow_consensus_decrease_percent: root
                .transaction_queue
                .slow_consensus_decrease_percent
                .unwrap_or(d.slow_consensus_decrease_percent),
            maximum_txn_per_account: root
                .transaction_queue
                .maximum_txn_per_account
                .unwrap_or(d.maximum_txn_per_account),
            minimum_last_ledger_buffer: root
                .transaction_queue
                .minimum_last_ledger_buffer
                .unwrap_or(d.minimum_last_ledger_buffer),
            zero_basefee_transaction_feelevel: root
                .transaction_queue
                .zero_basefee_transaction_feelevel
                .unwrap_or(d.zero_basefee_transaction_feelevel),
        };
        parsed.transaction_queue.validate_strict()?;
    }

    // ---- insight ----
    if let Some(ic) = root.insight {
        let d = InsightConfig::default();
        parsed.insight = InsightConfig {
            server: ic.server.unwrap_or(d.server),
            address: ic.address,
            prefix: ic.prefix,
        };
    }

    // ---- perf ----
    if let Some(pc) = root.perf {
        let d = PerfConfig::default();
        parsed.perf = PerfConfig {
            perf_log: pc.perf_log.map(RelPath::from),
            log_interval: pc.log_interval.unwrap_or(d.log_interval),
        };
    }

    // ---- ledger_tx_tables ----
    if let Some(lt) = root.ledger_tx_tables {
        let d = LedgerTxTablesConfig::default();
        parsed.ledger_tx_tables = LedgerTxTablesConfig {
            use_tx_tables: lt.use_tx_tables.unwrap_or(d.use_tx_tables),
        };
    }

    // ---- server + ports (with orphan / missing name checks) ----
    {
        // Build server config
        let server_defaults = PortDefaults::from(root.server.defaults);
        server_defaults.validate_strict("server.defaults")?;

        parsed.server = ServerConfig {
            port_names: root.server.ports.clone(),
            defaults: server_defaults,
        };

        // For each port table entry, check it's named in server.ports
        for name in root.port.keys() {
            if !root.server.ports.contains(name) {
                return Err(ConfigError::orphan_port_table(name));
            }
        }

        // Build each named port (missing tables → use defaults)
        for name in &root.server.ports {
            let port_cfg = if let Some(raw) = root.port.get(name) {
                let effective = PortDefaults {
                    ip: raw.ip.or(parsed.server.defaults.ip),
                    protocol: if raw.protocol.is_empty() {
                        parsed.server.defaults.protocol.clone()
                    } else {
                        raw.protocol.clone()
                    },
                    admin: if raw.admin.is_empty() {
                        parsed.server.defaults.admin.clone()
                    } else {
                        raw.admin.clone()
                    },
                    secure_gateway: if raw.secure_gateway.is_empty() {
                        parsed.server.defaults.secure_gateway.clone()
                    } else {
                        raw.secure_gateway.clone()
                    },
                    user: raw.user.clone().or_else(|| parsed.server.defaults.user.clone()),
                    password: raw
                        .password
                        .clone()
                        .or_else(|| parsed.server.defaults.password.clone()),
                    admin_user: raw
                        .admin_user
                        .clone()
                        .or_else(|| parsed.server.defaults.admin_user.clone()),
                    admin_password: raw
                        .admin_password
                        .clone()
                        .or_else(|| parsed.server.defaults.admin_password.clone()),
                    limit: raw.limit,
                    send_queue_limit: raw.send_queue_limit,
                    ssl_key: raw
                        .ssl_key
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_key.clone()),
                    ssl_cert: raw
                        .ssl_cert
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_cert.clone()),
                    ssl_chain: raw
                        .ssl_chain
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_chain.clone()),
                    ssl_ciphers: raw
                        .ssl_ciphers
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_ciphers.clone()),
                    ssl_cert_chain: raw
                        .ssl_cert_chain
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_cert_chain.clone()),
                    ssl_client_ca: raw
                        .ssl_client_ca
                        .clone()
                        .or_else(|| parsed.server.defaults.ssl_client_ca.clone()),
                    permessage_deflate: raw.permessage_deflate,
                    client_max_window_bits: raw.client_max_window_bits,
                    server_max_window_bits: raw.server_max_window_bits,
                    client_no_context_takeover: raw.client_no_context_takeover,
                    server_no_context_takeover: raw.server_no_context_takeover,
                    compress_level: raw.compress_level,
                    memory_level: raw.memory_level,
                };
                PortConfig {
                    name: name.clone(),
                    port: raw.port,
                    effective,
                }
            } else {
                PortConfig {
                    name: name.clone(),
                    port: 0,
                    effective: parsed.server.defaults.clone(),
                }
            };
            port_cfg.validate_strict()?;
            parsed.ports.insert(name.clone(), port_cfg);
        }
    }

    // Also keep voting_config in sync (Parsed has both voting and voting_config)
    parsed.voting_config = parsed.voting.clone();

    // ---- top-level cross-section validation ----
    parsed.validate_strict_toplevel()?;

    Ok(Config::new_with_parsed(parsed))
}

/// Convert the TomlSqliteConfig flat struct into the typed SqliteMode.
fn convert_sqlite(raw: TomlSqliteConfig) -> Result<SqliteConfig, ConfigError> {
    let has_safety = raw.safety_level.is_some();
    let has_tuning = raw.journal_mode.is_some()
        || raw.synchronous.is_some()
        || raw.temp_store.is_some()
        || raw.page_size.is_some();

    if has_safety && has_tuning {
        return Err(ConfigError::mutual_exclusion(
            "sqlite.safety_level",
            "sqlite.journal_mode/synchronous/temp_store/page_size",
        ));
    }

    let mode = if let Some(level) = raw.safety_level {
        SqliteMode::Safety { level }
    } else if has_tuning {
        SqliteMode::Tuning {
            journal_mode: raw.journal_mode,
            synchronous: raw.synchronous,
            temp_store: raw.temp_store,
            page_size: raw.page_size.unwrap_or(4096),
        }
    } else {
        SqliteMode::Default
    };

    Ok(SqliteConfig {
        mode,
        journal_size_limit: raw.journal_size_limit.unwrap_or(1_582_080),
    })
}

// ---------------------------------------------------------------------------
// Duration parsing helper (re-uses types::duration)
// ---------------------------------------------------------------------------

fn parse_amendment_majority_time(s: &str) -> Result<Duration, ConfigError> {
    crate::types::parse_amendment_majority_time(s, true)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_text: &str) -> Result<Config, ConfigError> {
        super::super::parse_toml(toml_text)
    }

    // ---- happy-path tests ----

    #[test]
    fn minimal_empty_config() {
        let cfg = parse("").expect("empty TOML should parse");
        assert_eq!(cfg.network_id(), 0);
    }

    #[test]
    fn network_scalars() {
        let cfg = parse(
            r#"
            network_id = 1
            network_quorum = 5
            "#,
        )
        .unwrap();
        assert_eq!(cfg.network_id(), 1);
        assert_eq!(cfg.network_quorum(), 5);
    }

    #[test]
    fn overlay_defaults_pass_validation() {
        // Default values for overlay are within range; should parse fine.
        let cfg = parse(
            r#"
            [overlay]
            max_unknown_time = 600
            max_diverged_time = 300
            "#,
        )
        .unwrap();
        assert_eq!(cfg.overlay().max_unknown_time, 600);
        assert_eq!(cfg.overlay().max_diverged_time, 300);
    }

    #[test]
    fn port_table() {
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc_admin"]

            [port.rpc_admin]
            port = 5005
            protocol = ["Http"]
            "#,
        )
        .unwrap();
        let p = cfg.port("rpc_admin").expect("port should exist");
        assert_eq!(p.port, 5005);
    }

    #[test]
    fn ips_array() {
        let cfg = parse(r#"ips = ["r.ripple.com 51235"]"#).unwrap();
        assert_eq!(cfg.ips().len(), 1);
    }

    #[test]
    fn features_set() {
        let cfg = parse(r#"features = ["Flow", "TickSize"]"#).unwrap();
        assert!(cfg.features().contains("Flow"));
        assert!(cfg.features().contains("TickSize"));
    }

    #[test]
    fn sqlite_safety_mode() {
        let cfg = parse(r#"[sqlite]
safety_level = "High"
"#)
        .unwrap();
        assert!(matches!(
            cfg.sqlite().mode,
            SqliteMode::Safety { level: SqliteSafety::High }
        ));
    }

    // ---- error cases ----

    #[test]
    fn overlay_max_unknown_time_out_of_range() {
        let err = parse(
            r#"
            [overlay]
            max_unknown_time = 100
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err.kind,
            crate::ConfigErrorKind::OutOfRange { .. }
        ));
    }

    #[test]
    fn overlay_max_diverged_time_too_large() {
        let err = parse(
            r#"
            [overlay]
            max_diverged_time = 9999
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err.kind,
            crate::ConfigErrorKind::OutOfRange { .. }
        ));
    }

    #[test]
    fn orphan_port_table_error() {
        let err = parse(
            r#"
            [server]
            ports = []

            [port.rogue]
            port = 1234
            protocol = ["Http"]
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err.kind,
            crate::ConfigErrorKind::OrphanPortTable { .. }
        ));
    }

    #[test]
    fn validation_seed_and_token_mutual_exclusion() {
        let err = parse(
            r#"
            validation_seed = "abc"
            validator_token = "def"
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err.kind,
            crate::ConfigErrorKind::MutualExclusion { .. }
        ));
    }

    #[test]
    fn sqlite_mutual_exclusion() {
        let err = parse(
            r#"
            [sqlite]
            safety_level = "High"
            journal_mode = "Wal"
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err.kind,
            crate::ConfigErrorKind::MutualExclusion { .. }
        ));
    }

    #[test]
    fn unknown_top_level_key_is_error() {
        let err = parse(r#"totally_unknown_key = 42"#).unwrap_err();
        // The toml crate produces a parse error for deny_unknown_fields
        // wrapped in our Grammar/Lex error.
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected message: {msg}"
        );
    }

    // ---- additional happy-path tests ----

    #[test]
    fn minimal_config_defaults_populated() {
        let cfg = parse("").unwrap();
        // Check that defaults are sensibly populated
        assert_eq!(cfg.overlay().max_unknown_time, 600);
        assert_eq!(cfg.overlay().max_diverged_time, 300);
        assert_eq!(cfg.max_transactions(), 250);
        assert!(!cfg.compression());
        assert!(!cfg.standalone());
    }

    #[test]
    fn all_boolean_flags() {
        let cfg = parse(
            r#"
            compression = true
            ledger_replay = true
            beta_rpc_api = true
            signing_enabled = true
            elb_support = true
            peer_private = true
            "#,
        )
        .unwrap();
        assert!(cfg.compression());
        assert!(cfg.ledger_replay());
        assert!(cfg.beta_rpc_api());
        assert!(cfg.signing_enabled());
        assert!(cfg.elb_support());
        assert!(cfg.peer_private());
    }

    #[test]
    fn server_domain_set() {
        let cfg = parse(r#"server_domain = "example.com""#).unwrap();
        assert_eq!(cfg.server_domain(), Some("example.com"));
    }

    #[test]
    fn validators_array_inline_tables() {
        let cfg = parse(
            r#"validators = [
                { key = "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu", label = "Validator One" },
                { key = "nHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK" },
            ]"#,
        )
        .unwrap();
        let vs = cfg.trusted_validators();
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0].key, "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu");
        assert_eq!(vs[0].label.as_deref(), Some("Validator One"));
        assert!(vs[1].label.is_none());
    }

    #[test]
    fn features_set_deduplicates() {
        // Features is a HashSet so duplicates are dropped
        let cfg = parse(r#"features = ["Flow", "Flow", "TickSize"]"#).unwrap();
        assert_eq!(cfg.features().len(), 2);
    }

    #[test]
    fn crawl_detailed_form() {
        let cfg = parse(
            r#"
            [crawl]
            overlay = true
            server = true
            counts = false
            unl = false
            "#,
        )
        .unwrap();
        assert!(matches!(
            cfg.crawl(),
            CrawlConfig::Detailed { overlay: true, server: true, counts: false, unl: false }
        ));
    }

    #[test]
    fn crawl_unknown_field_is_error() {
        let err = parse(
            r#"
            [crawl]
            unknown_crawl_key = true
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn crawl_default_is_all_false() {
        let cfg = parse("").unwrap();
        assert!(matches!(
            cfg.crawl(),
            CrawlConfig::Detailed { overlay: false, server: false, counts: false, unl: false }
        ));
    }

    #[test]
    fn sqlite_tuning_mode_page_size() {
        let cfg = parse(
            r#"
            [sqlite]
            journal_mode = "Wal"
            page_size = 4096
            "#,
        )
        .unwrap();
        assert!(matches!(
            cfg.sqlite().mode,
            SqliteMode::Tuning { page_size: 4096, .. }
        ));
    }

    #[test]
    fn sqlite_default_mode() {
        let cfg = parse("").unwrap();
        assert!(matches!(cfg.sqlite().mode, SqliteMode::Default));
    }

    #[test]
    fn overlay_unknown_field_is_error() {
        let err = parse(
            r#"
            [overlay]
            unknown_overlay_key = 123
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn reduce_relay_unknown_field_is_error() {
        let err = parse(
            r#"
            [reduce_relay]
            bogus_key = true
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn node_db_unknown_field_is_error() {
        let err = parse(
            r#"
            [node_db]
            unknown_field = "nope"
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn sqlite_unknown_field_is_error() {
        let err = parse(
            r#"
            [sqlite]
            bogus_sqlite_key = 1
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn port_unknown_field_is_error() {
        let err = parse(
            r#"
            [server]
            ports = ["rpc"]

            [port.rpc]
            port = 5005
            bogus_port_field = "nope"
            "#,
        )
        .unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("unknown") || msg.contains("TOML") || msg.contains("parse"),
            "unexpected: {msg}"
        );
    }

    // ---- range-check error tests ----

    #[test]
    fn reduce_relay_tx_relay_percentage_too_low() {
        let err = parse(
            r#"
            [reduce_relay]
            tx_relay_percentage = 5
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("tx_relay_percentage"), "unexpected: {msg}");
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_too_high() {
        let err = parse(
            r#"
            [reduce_relay]
            tx_relay_percentage = 101
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_boundary_low_ok() {
        // 10 is the minimum allowed
        let cfg = parse(
            r#"
            [reduce_relay]
            tx_relay_percentage = 10
            "#,
        )
        .unwrap();
        assert_eq!(cfg.reduce_relay().tx_relay_percentage, 10);
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_boundary_high_ok() {
        // 100 is the maximum allowed
        let cfg = parse(
            r#"
            [reduce_relay]
            tx_relay_percentage = 100
            "#,
        )
        .unwrap();
        assert_eq!(cfg.reduce_relay().tx_relay_percentage, 100);
    }

    #[test]
    fn node_db_earliest_seq_zero_is_error() {
        let err = parse(
            r#"
            [node_db]
            earliest_seq = 0
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("earliest_seq"), "unexpected: {msg}");
    }

    #[test]
    fn node_db_nudb_block_size_non_power_of_two() {
        let err = parse(
            r#"
            [node_db]
            nudb_block_size = 5000
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("nudb_block_size"), "unexpected: {msg}");
    }

    #[test]
    fn node_db_nudb_block_size_too_small() {
        let err = parse(
            r#"
            [node_db]
            nudb_block_size = 512
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn node_db_nudb_block_size_too_large() {
        let err = parse(
            r#"
            [node_db]
            nudb_block_size = 65536
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn node_db_valid_nudb_block_size() {
        let cfg = parse(
            r#"
            [node_db]
            nudb_block_size = 4096
            "#,
        )
        .unwrap();
        assert_eq!(cfg.node_db().nudb_block_size, 4096);
    }

    #[test]
    fn sqlite_page_size_non_power_of_two() {
        let err = parse(
            r#"
            [sqlite]
            page_size = 1000
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("page_size"), "unexpected: {msg}");
    }

    #[test]
    fn sqlite_page_size_valid_power_of_two() {
        let cfg = parse(
            r#"
            [sqlite]
            page_size = 4096
            "#,
        )
        .unwrap();
        assert!(matches!(
            cfg.sqlite().mode,
            SqliteMode::Tuning { page_size: 4096, .. }
        ));
    }

    #[test]
    fn sqlite_safety_and_synchronous_mutual_exclusion() {
        // safety_level + synchronous is also mutual exclusion (has_tuning includes synchronous)
        let err = parse(
            r#"
            [sqlite]
            safety_level = "Low"
            synchronous = "Full"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::MutualExclusion { .. }));
    }

    #[test]
    fn sqlite_safety_and_page_size_mutual_exclusion() {
        let err = parse(
            r#"
            [sqlite]
            safety_level = "High"
            page_size = 4096
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::MutualExclusion { .. }));
    }

    #[test]
    fn port_zero_is_error() {
        let err = parse(
            r#"
            [server]
            ports = ["bad_port"]

            [port.bad_port]
            port = 0
            protocol = ["Http"]
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("port"), "unexpected: {msg}");
    }

    #[test]
    fn port_compress_level_too_high() {
        let err = parse(
            r#"
            [server]
            ports = ["rpc"]

            [port.rpc]
            port = 5005
            compress_level = 10
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("compress_level"), "unexpected: {msg}");
    }

    #[test]
    fn port_compress_level_boundary_ok() {
        // 9 is the maximum
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc"]

            [port.rpc]
            port = 5005
            compress_level = 9
            "#,
        )
        .unwrap();
        let p = cfg.port("rpc").unwrap();
        assert_eq!(p.effective.compress_level, 9);
    }

    #[test]
    fn port_listed_but_no_table_uses_defaults() {
        // server.ports lists "rpc" but no [port.rpc] table exists.
        // According to the impl this creates a PortConfig with port=0 then validates,
        // which should fail the port>0 check.
        let err = parse(
            r#"
            [server]
            ports = ["rpc"]
            "#,
        )
        .unwrap_err();
        // port=0 because no table provided
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn port_ip_inherited_from_server_defaults() {
        // The `ip` field uses Option so server defaults merge when port doesn't specify it
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc"]
            ip = "127.0.0.1"

            [port.rpc]
            port = 5005
            "#,
        )
        .unwrap();
        let p = cfg.port("rpc").unwrap();
        // ip from server defaults should be merged since port didn't specify it
        assert_eq!(
            p.effective.ip,
            Some("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        );
    }

    #[test]
    fn port_ssl_key_overrides_server_defaults() {
        // ssl_key is an Option<PathBuf>, so server defaults are merged when port doesn't specify
        // and per-port value overrides when set
        use std::path::PathBuf;
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc", "ws"]
            ssl_key = "/etc/ssl/server.key"

            [port.rpc]
            port = 5005
            ssl_key = "/etc/ssl/custom.key"

            [port.ws]
            port = 6005
            "#,
        )
        .unwrap();
        let rpc = cfg.port("rpc").unwrap();
        let ws = cfg.port("ws").unwrap();
        // rpc explicitly sets ssl_key → that takes precedence
        assert_eq!(rpc.effective.ssl_key, Some(PathBuf::from("/etc/ssl/custom.key")));
        // ws doesn't set ssl_key → server default is inherited
        assert_eq!(ws.effective.ssl_key, Some(PathBuf::from("/etc/ssl/server.key")));
    }

    // ---- cross-section / Parsed::validate_strict_toplevel ----

    #[test]
    fn max_transactions_too_low() {
        let err = parse(r#"max_transactions = 50"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("max_transactions"), "unexpected: {msg}");
    }

    #[test]
    fn max_transactions_too_high() {
        let err = parse(r#"max_transactions = 1001"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn max_transactions_boundary_ok() {
        let cfg = parse(r#"max_transactions = 100"#).unwrap();
        assert_eq!(cfg.max_transactions(), 100);
    }

    #[test]
    fn fetch_depth_count_too_small_via_tagged() {
        // FetchDepth::Count(5) expressed in TOML tagged form — 5 < 10 → OutOfRange
        let err = parse(r#"fetch_depth = { "Count" = 5 }"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("fetch_depth"), "unexpected: {msg}");
    }

    #[test]
    fn fetch_depth_count_boundary_ok_via_tagged() {
        // FetchDepth::Count(10) — at boundary, should pass
        let cfg = parse(r#"fetch_depth = { "Count" = 10 }"#).unwrap();
        assert!(matches!(cfg.fetch_depth(), FetchDepth::Count(10)));
    }

    #[test]
    fn fetch_depth_full_via_tagged() {
        // FetchDepth::Full
        let cfg = parse(r#"fetch_depth = "Full""#).unwrap();
        assert!(matches!(cfg.fetch_depth(), FetchDepth::Full));
    }

    #[test]
    fn fetch_depth_none_via_tagged() {
        // FetchDepth::None_
        let cfg = parse(r#"fetch_depth = "None_""#).unwrap();
        assert!(matches!(cfg.fetch_depth(), FetchDepth::None_));
    }

    #[test]
    fn workers_zero_is_ok() {
        // 0 means "auto" — allowed
        let cfg = parse(r#"workers = 0"#).unwrap();
        assert_eq!(cfg.workers(), 0);
    }

    #[test]
    fn workers_too_large() {
        let err = parse(r#"workers = 2000"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("workers"), "unexpected: {msg}");
    }

    #[test]
    fn workers_boundary_ok() {
        let cfg = parse(r#"workers = 1024"#).unwrap();
        assert_eq!(cfg.workers(), 1024);
    }

    #[test]
    fn sweep_interval_too_small() {
        let err = parse(r#"sweep_interval = 5"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("sweep_interval"), "unexpected: {msg}");
    }

    #[test]
    fn sweep_interval_boundary_ok() {
        let cfg = parse(r#"sweep_interval = 10"#).unwrap();
        assert_eq!(cfg.sweep_interval(), Some(10));
    }

    #[test]
    fn sweep_interval_too_large() {
        let err = parse(r#"sweep_interval = 601"#).unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    #[test]
    fn validator_list_threshold_exceeds_keys_count() {
        // threshold=2 but only 1 key → error
        let err = parse(
            r#"
            validator_list_keys = ["nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu"]
            validator_list_threshold = 2
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::Cross { .. }));
        let msg = err.message();
        assert!(msg.contains("validator_list_threshold"), "unexpected: {msg}");
    }

    #[test]
    fn validator_list_threshold_equal_keys_count_ok() {
        // threshold = count → ok
        let cfg = parse(
            r#"
            validator_list_keys = ["nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu"]
            validator_list_threshold = 1
            "#,
        )
        .unwrap();
        assert_eq!(cfg.validator_list_threshold(), Some(1));
    }

    #[test]
    fn network_quorum_exceeds_peers_max() {
        // network_quorum=10 with peers_max=5 → error
        let err = parse(
            r#"
            peers_max = 5
            network_quorum = 10
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::Cross { .. }));
        let msg = err.message();
        assert!(msg.contains("network_quorum"), "unexpected: {msg}");
    }

    #[test]
    fn network_quorum_equal_peers_max_ok() {
        let cfg = parse(
            r#"
            peers_max = 10
            network_quorum = 10
            "#,
        )
        .unwrap();
        assert_eq!(cfg.network_quorum(), 10);
    }

    // ---- CrawlConfig LegacyBool unrepresentable in TOML ----

    #[test]
    fn crawl_bool_literal_is_deserialise_error() {
        // [crawl] section cannot be a bare boolean in TOML — it's a table
        // This should produce a TOML parse error
        let err = parse(
            r#"
            crawl = true
            "#,
        )
        .unwrap_err();
        // Should be a grammar/parse error since crawl must be a table in TOML
        let msg = err.message();
        assert!(
            msg.contains("parse") || msg.contains("TOML") || msg.contains("error") || msg.contains("invalid"),
            "unexpected: {msg}"
        );
    }

    // ---- features top-level array ----

    #[test]
    fn features_empty_array() {
        let cfg = parse(r#"features = []"#).unwrap();
        assert!(cfg.features().is_empty());
    }

    #[test]
    fn features_single_element() {
        let cfg = parse(r#"features = ["Flow"]"#).unwrap();
        assert!(cfg.features().contains("Flow"));
        assert_eq!(cfg.features().len(), 1);
    }

    // ---- vl config ----

    #[test]
    fn vl_enabled() {
        let cfg = parse(
            r#"
            [vl]
            enabled = true
            "#,
        )
        .unwrap();
        assert!(cfg.vl().enabled);
    }

    #[test]
    fn vl_default_disabled() {
        let cfg = parse("").unwrap();
        assert!(!cfg.vl().enabled);
    }

    // ---- multiple ports ----

    #[test]
    fn multiple_ports_happy_path() {
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc", "peer"]

            [port.rpc]
            port = 5005
            protocol = ["Http"]

            [port.peer]
            port = 51235
            protocol = ["Peer"]
            "#,
        )
        .unwrap();
        assert!(cfg.port("rpc").is_some());
        assert!(cfg.port("peer").is_some());
        assert_eq!(cfg.port("rpc").unwrap().port, 5005);
        assert_eq!(cfg.port("peer").unwrap().port, 51235);
    }

    #[test]
    fn port_protocols_inherited_from_server_defaults() {
        let cfg = parse(
            r#"
            [server]
            ports = ["rpc"]
            protocol = ["Http"]

            [port.rpc]
            port = 5005
            "#,
        )
        .unwrap();
        let p = cfg.port("rpc").unwrap();
        assert_eq!(p.effective.protocol, vec![PortProtocol::Http]);
    }

    // ---- overlay boundary values ----

    #[test]
    fn overlay_max_unknown_time_boundary_min() {
        // 300 is the minimum
        let cfg = parse(
            r#"
            [overlay]
            max_unknown_time = 300
            "#,
        )
        .unwrap();
        assert_eq!(cfg.overlay().max_unknown_time, 300);
    }

    #[test]
    fn overlay_max_unknown_time_boundary_max() {
        // 1800 is the maximum
        let cfg = parse(
            r#"
            [overlay]
            max_unknown_time = 1800
            "#,
        )
        .unwrap();
        assert_eq!(cfg.overlay().max_unknown_time, 1800);
    }

    #[test]
    fn overlay_max_diverged_time_boundary_min() {
        // 60 is the minimum
        let cfg = parse(
            r#"
            [overlay]
            max_diverged_time = 60
            "#,
        )
        .unwrap();
        assert_eq!(cfg.overlay().max_diverged_time, 60);
    }

    #[test]
    fn overlay_max_diverged_time_boundary_max() {
        // 900 is the maximum
        let cfg = parse(
            r#"
            [overlay]
            max_diverged_time = 900
            "#,
        )
        .unwrap();
        assert_eq!(cfg.overlay().max_diverged_time, 900);
    }

    #[test]
    fn overlay_max_diverged_time_too_low() {
        let err = parse(
            r#"
            [overlay]
            max_diverged_time = 59
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
    }

    // ---- import_db ----

    #[test]
    fn import_db_valid() {
        let cfg = parse(
            r#"
            [import_db]
            kind = "NuDb"
            path = "/tmp/import"
            "#,
        )
        .unwrap();
        assert!(cfg.import_db().is_some());
    }

    #[test]
    fn import_db_earliest_seq_zero_is_error() {
        let err = parse(
            r#"
            [import_db]
            earliest_seq = 0
            "#,
        )
        .unwrap_err();
        assert!(matches!(err.kind, crate::ConfigErrorKind::OutOfRange { .. }));
        let msg = err.message();
        assert!(msg.contains("earliest_seq"), "unexpected: {msg}");
    }

    // ---- voting config ----

    #[test]
    fn voting_config_set() {
        let cfg = parse(
            r#"
            [voting]
            reference_fee = 12
            account_reserve = 10000000
            owner_reserve = 2000000
            "#,
        )
        .unwrap();
        let v = cfg.voting();
        assert_eq!(v.reference_fee, 12);
        assert_eq!(v.account_reserve, 10000000);
        assert_eq!(v.owner_reserve, 2000000);
    }

    #[test]
    fn fee_default_overrides_voting_reference_fee() {
        let cfg = parse(
            r#"
            fee_default = 99
            [voting]
            reference_fee = 10
            "#,
        )
        .unwrap();
        // fee_default overrides voting.reference_fee
        assert_eq!(cfg.voting().reference_fee, 99);
    }

    // ---- ips / ips_fixed ----

    #[test]
    fn ips_fixed_array() {
        let cfg = parse(r#"ips_fixed = ["r.ripple.com 51235"]"#).unwrap();
        assert_eq!(cfg.ips_fixed().len(), 1);
    }

    // ---- ssl_verify ----

    #[test]
    fn ssl_verify_false() {
        let cfg = parse(r#"ssl_verify = false"#).unwrap();
        assert!(!cfg.ssl_verify());
    }

    #[test]
    fn ssl_verify_default_true() {
        let cfg = parse("").unwrap();
        assert!(cfg.ssl_verify());
    }
}

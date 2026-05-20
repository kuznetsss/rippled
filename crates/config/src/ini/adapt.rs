//! Stage-2 INI adapter: dispatch `RawSections` → typed `Config`.
//!
//! Each `RawSection` is routed to one of three handler categories:
//! - Category 1: pure-kv → `serde::from_kv_section::<T>`.
//! - Category 2: bare-line list → `serde::from_bare_lines::<Vec<T>>`.
//! - Category 3: special-shape (handwritten adapters).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config::{Config, Parsed};
use crate::error::ConfigError;
use crate::types::*;
use crate::types::path::RelPath;

use super::grammar::{parse_ini_bool, parse_ini_int};
use super::raw::{RawLineKind, RawSection, RawSections};
use super::serde::{from_bare_lines, from_kv_section};

// ---------------------------------------------------------------------------
// Handwritten port-section adapter (replaces PortConfigProxy + flatten)
// ---------------------------------------------------------------------------
//
// We cannot use #[serde(flatten)] for PortDefaults because serde's flatten
// machinery calls `deserialize_any`, which our INI ValueDeserializer answers
// with `visit_str`. The Vec<IpNet> and Vec<PortProtocol> visitors then reject
// a bare string because they only handle sequences. Instead we walk the kv
// map explicitly and parse each known field ourselves.
fn adapt_port_section(sec: &RawSection) -> Result<PortConfig, ConfigError> {
    use std::net::IpAddr;
    use std::str::FromStr;
    use ipnet::IpNet;

    let map = sec.lookup();
    let mut pc = PortConfig::default();

    for (key, value) in &map {
        let v: &str = value;
        match *key {
            "port" => {
                pc.port = parse_ini_int(v)?;
            }
            "ip" => {
                pc.effective.ip = Some(IpAddr::from_str(v.trim()).map_err(|_| {
                    ConfigError::grammar("port.ip", v, "invalid IP address")
                })?);
            }
            "protocol" => {
                // Comma-separated list of protocols (e.g. "http,https" or "peer").
                for proto_str in v.split(',') {
                    let proto_str = proto_str.trim().to_ascii_lowercase();
                    let proto = match proto_str.as_str() {
                        "http" => PortProtocol::Http,
                        "https" => PortProtocol::Https,
                        "ws" => PortProtocol::Ws,
                        "wss" => PortProtocol::Wss,
                        "peer" => PortProtocol::Peer,
                        "grpc" => PortProtocol::Grpc,
                        other => return Err(ConfigError::grammar(
                            "port.protocol", other, "expected http, https, ws, wss, peer, or grpc",
                        )),
                    };
                    pc.effective.protocol.push(proto);
                }
            }
            "admin" => {
                // Comma-separated CIDR list (or single IP/CIDR).
                for net_str in v.split(',') {
                    let net_str = net_str.trim();
                    let net = IpNet::from_str(net_str)
                        .or_else(|_| {
                            // Plain IP address — treat as host route.
                            IpAddr::from_str(net_str)
                                .map(|ip| if ip.is_ipv4() {
                                    IpNet::from_str(&format!("{}/32", ip)).unwrap()
                                } else {
                                    IpNet::from_str(&format!("{}/128", ip)).unwrap()
                                })
                        })
                        .map_err(|_| ConfigError::grammar("port.admin", net_str, "invalid CIDR/IP"))?;
                    pc.effective.admin.push(net);
                }
            }
            "secure_gateway" => {
                for net_str in v.split(',') {
                    let net_str = net_str.trim();
                    let net = IpNet::from_str(net_str)
                        .or_else(|_| {
                            IpAddr::from_str(net_str)
                                .map(|ip| if ip.is_ipv4() {
                                    IpNet::from_str(&format!("{}/32", ip)).unwrap()
                                } else {
                                    IpNet::from_str(&format!("{}/128", ip)).unwrap()
                                })
                        })
                        .map_err(|_| ConfigError::grammar("port.secure_gateway", net_str, "invalid CIDR/IP"))?;
                    pc.effective.secure_gateway.push(net);
                }
            }
            "user" => { pc.effective.user = Some(v.to_owned()); }
            "password" => { pc.effective.password = Some(v.to_owned()); }
            "admin_user" => { pc.effective.admin_user = Some(v.to_owned()); }
            "admin_password" => { pc.effective.admin_password = Some(v.to_owned()); }
            "limit" => {
                pc.effective.limit = if v.trim().to_ascii_lowercase() == "unlimited" {
                    PortLimit::Unlimited
                } else {
                    PortLimit::Count(parse_ini_int(v)?)
                };
            }
            "send_queue_limit" => { pc.effective.send_queue_limit = parse_ini_int(v)?; }
            "ssl_key" => { pc.effective.ssl_key = Some(PathBuf::from(v)); }
            "ssl_cert" => { pc.effective.ssl_cert = Some(PathBuf::from(v)); }
            "ssl_chain" => { pc.effective.ssl_chain = Some(PathBuf::from(v)); }
            "ssl_ciphers" => { pc.effective.ssl_ciphers = Some(v.to_owned()); }
            "ssl_cert_chain" => { pc.effective.ssl_cert_chain = Some(PathBuf::from(v)); }
            "ssl_client_ca" => { pc.effective.ssl_client_ca = Some(PathBuf::from(v)); }
            "permessage_deflate" => { pc.effective.permessage_deflate = parse_ini_bool(v)?; }
            "client_max_window_bits" => { pc.effective.client_max_window_bits = parse_ini_int(v)?; }
            "server_max_window_bits" => { pc.effective.server_max_window_bits = parse_ini_int(v)?; }
            "client_no_context_takeover" => { pc.effective.client_no_context_takeover = parse_ini_bool(v)?; }
            "server_no_context_takeover" => { pc.effective.server_no_context_takeover = parse_ini_bool(v)?; }
            "compress_level" => { pc.effective.compress_level = parse_ini_int(v)?; }
            "memory_level" => { pc.effective.memory_level = parse_ini_int(v)?; }
            // Unknown port-level keys are silently dropped (lenient INI mode).
            _ => {}
        }
    }

    Ok(pc)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(super) fn adapt(raw: RawSections) -> Result<Config, ConfigError> {
    let mut p = Parsed::default();

    // First pass: handle everything except [port_*] sections.
    // We need [server] to know port names before we can handle [port_*].
    let mut server_done = false;

    for sec in &raw.sections {
        dispatch_section(sec, &mut p, &raw)?;
        if sec.name == "server" {
            server_done = true;
        }
    }

    // Second pass: handle [port_*] sections now that server.port_names is known.
    // Port names in [server] are the full section names (e.g. "port_rpc_admin_local"),
    // so we look for sections with exactly that name — not "port_<name>".
    if server_done {
        let port_names: Vec<String> = p.server.port_names.clone();
        for name in &port_names {
            // The section name IS the port name (e.g. [port_rpc_admin_local]).
            if let Some(port_sec) = raw.first_named(name) {
                let mut pc: PortConfig = adapt_port_section(port_sec)?;
                pc.name = name.clone();
                // Apply server-level defaults for fields not set in the port section.
                apply_port_defaults(&mut pc, &p.server.defaults);
                p.ports.insert(name.clone(), pc);
            }
        }
    }

    Ok(Config::new_with_parsed(p))
}

/// Apply server-level `PortDefaults` to a `PortConfig` for fields that are still at zero/empty defaults.
/// The port-level config takes precedence; server defaults only fill in what's missing.
fn apply_port_defaults(pc: &mut PortConfig, defaults: &PortDefaults) {
    // Only apply default IP if port config has no IP.
    if pc.effective.ip.is_none() {
        pc.effective.ip = defaults.ip;
    }
    if pc.effective.protocol.is_empty() {
        pc.effective.protocol = defaults.protocol.clone();
    }
    if pc.effective.admin.is_empty() {
        pc.effective.admin = defaults.admin.clone();
    }
    if pc.effective.secure_gateway.is_empty() {
        pc.effective.secure_gateway = defaults.secure_gateway.clone();
    }
    if pc.effective.user.is_none() {
        pc.effective.user = defaults.user.clone();
    }
    if pc.effective.password.is_none() {
        pc.effective.password = defaults.password.clone();
    }
    if pc.effective.admin_user.is_none() {
        pc.effective.admin_user = defaults.admin_user.clone();
    }
    if pc.effective.admin_password.is_none() {
        pc.effective.admin_password = defaults.admin_password.clone();
    }
    if matches!(pc.effective.limit, PortLimit::Unlimited) {
        pc.effective.limit = defaults.limit;
    }
    if pc.effective.ssl_key.is_none() {
        pc.effective.ssl_key = defaults.ssl_key.clone();
    }
    if pc.effective.ssl_cert.is_none() {
        pc.effective.ssl_cert = defaults.ssl_cert.clone();
    }
    if pc.effective.ssl_chain.is_none() {
        pc.effective.ssl_chain = defaults.ssl_chain.clone();
    }
    if pc.effective.ssl_ciphers.is_none() {
        pc.effective.ssl_ciphers = defaults.ssl_ciphers.clone();
    }
    if pc.effective.ssl_cert_chain.is_none() {
        pc.effective.ssl_cert_chain = defaults.ssl_cert_chain.clone();
    }
    if pc.effective.ssl_client_ca.is_none() {
        pc.effective.ssl_client_ca = defaults.ssl_client_ca.clone();
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn dispatch_section(sec: &RawSection, p: &mut Parsed, _raw: &RawSections) -> Result<(), ConfigError> {
    match sec.name.as_str() {
        // ---- Category 1: pure kv sections ----
        "overlay" => {
            p.overlay = from_kv_section(sec)?;
            p.overlay.validate_lenient();
        }
        "node_db" => {
            p.node_db = adapt_node_db(sec)?;
            p.node_db.validate_lenient();
        }
        "import_db" => {
            let mut db = adapt_node_db(sec)?;
            db.validate_lenient();
            p.import_db = Some(db);
        }
        "sqlite" => {
            p.sqlite = adapt_sqlite(sec)?;
        }
        "transaction_queue" => {
            p.transaction_queue = from_kv_section(sec)?;
            p.transaction_queue.validate_lenient();
        }
        "insight" => {
            p.insight = adapt_insight(sec)?;
        }
        "perf" => {
            p.perf = from_kv_section(sec)?;
        }
        "ledger_tx_tables" => {
            p.ledger_tx_tables = from_kv_section(sec)?;
        }
        "reduce_relay" => {
            p.reduce_relay = from_kv_section(sec)?;
            p.reduce_relay.validate_lenient();
        }
        "vl" => {
            p.vl = from_kv_section(sec)?;
        }
        "voting" => {
            p.voting = from_kv_section(sec)?;
        }

        // ---- Category 2: bare-line lists ----
        "ips" => {
            p.ips = from_bare_lines(sec)?;
        }
        "ips_fixed" => {
            p.ips_fixed = from_bare_lines(sec)?;
        }
        "sntp_servers" => {
            p.sntp_servers = from_bare_lines(sec)?;
        }
        "cluster_nodes" => {
            p.cluster_nodes = adapt_cluster_nodes(sec)?;
        }
        "features" => {
            let names: Vec<String> = from_bare_lines(sec)?;
            p.features.extend(names);
        }
        "validators" => {
            let vs: Vec<TrustedValidator> = adapt_trusted_validators(sec)?;
            p.trusted_validators.extend(vs);
        }
        "validator_keys" => {
            let vs: Vec<TrustedValidator> = adapt_trusted_validators(sec)?;
            p.trusted_validators.extend(vs);
        }
        "amendments" => {
            p.amendments = adapt_known_amendments(sec)?;
        }
        "veto_amendments" => {
            p.veto_amendments = adapt_known_amendments(sec)?;
        }
        "validator_list_sites" => {
            p.validator_list_sites = from_bare_lines(sec)?;
        }
        "validator_list_keys" => {
            p.validator_list_keys = from_bare_lines(sec)?;
        }
        "rpc_startup" => {
            p.rpc_startup = adapt_rpc_startup(sec)?;
        }

        // ---- Category 3: special-shape sections ----
        "server" => {
            adapt_server(sec, p)?;
        }
        "crawl" => {
            p.crawl = adapt_crawl(sec)?;
        }

        // Single-bare-line sections
        "database_path" => {
            p.database_path = Some(RelPath(PathBuf::from(adapt_single_line(sec)?)));
        }
        "debug_logfile" => {
            p.debug_logfile = Some(RelPath(PathBuf::from(adapt_single_line(sec)?)));
        }
        "validators_file" => {
            p.validators_file = Some(RelPath(PathBuf::from(adapt_single_line(sec)?)));
        }
        "node_size" => {
            p.node_size = Some(adapt_node_size(sec)?);
        }
        "network_id" => {
            p.network_id = adapt_network_id(sec)?;
        }
        "network_quorum" => {
            p.network_quorum = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "peer_private" => {
            p.peer_private = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "peers_max" => {
            p.peers_max = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "peers_in_max" => {
            p.peers_in_max = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "peers_out_max" => {
            p.peers_out_max = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "ledger_history" => {
            p.ledger_history = adapt_ledger_history(sec)?;
        }
        "fetch_depth" => {
            p.fetch_depth = adapt_fetch_depth(sec)?;
        }
        "max_transactions" => {
            let v: i32 = parse_ini_int(&adapt_single_line(sec)?)?;
            // Clamp per design §5.2 / analysis §5.
            p.max_transactions = v.clamp(100, 1000);
        }
        "amendment_majority_time" => {
            p.amendment_majority_time =
                parse_amendment_majority_time(&adapt_single_line(sec)?, false)?;
        }
        "workers" => {
            p.workers = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "io_workers" => {
            p.io_workers = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "prefetch_workers" => {
            p.prefetch_workers = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "sweep_interval" => {
            p.sweep_interval = Some(parse_ini_int(&adapt_single_line(sec)?)?);
        }
        "server_domain" => {
            p.server_domain = Some(adapt_single_line(sec)?);
        }
        "compression" => {
            p.compression = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "ledger_replay" => {
            p.ledger_replay = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "beta_rpc_api" => {
            p.beta_rpc_api = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "signing_support" => {
            p.signing_enabled = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "elb_support" => {
            p.elb_support = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "ssl_verify" => {
            p.ssl_verify = parse_ini_bool(&adapt_single_line(sec)?)?;
        }
        "ssl_verify_file" => {
            p.ssl_verify_file = Some(PathBuf::from(adapt_single_line(sec)?));
        }
        "ssl_verify_dir" => {
            p.ssl_verify_dir = Some(PathBuf::from(adapt_single_line(sec)?));
        }
        "fee_default" => {
            p.fee_default = Some(parse_ini_int(&adapt_single_line(sec)?)?);
        }
        "path_search" => {
            p.path_search = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "path_search_old" => {
            p.path_search_old = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "path_search_fast" => {
            p.path_search_fast = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "path_search_max" => {
            p.path_search_max = parse_ini_int(&adapt_single_line(sec)?)?;
        }
        "relay_validations" => {
            p.relay_untrusted_validations = adapt_relay_policy(sec)?;
        }
        "relay_proposals" => {
            p.relay_untrusted_proposals = adapt_relay_policy(sec)?;
        }
        "validator_list_threshold" => {
            p.validator_list_threshold = Some(parse_ini_int(&adapt_single_line(sec)?)?);
        }
        "websocket_ping_frequency" => {
            p.websocket_ping_frequency = Some(parse_ini_int(&adapt_single_line(sec)?)?);
        }

        // Multi-line blob sections
        "validation_seed" => {
            p.validation_seed = Some(adapt_multi_line_blob(sec)?);
        }
        "validator_token" => {
            p.validator_token = Some(adapt_multi_line_blob(sec)?);
        }
        "validator_key_revocation" => {
            p.validator_key_revocation = Some(adapt_multi_line_blob(sec)?);
        }

        // [port_*] sections are handled in the second pass after [server].
        //
        // The synthetic "__preamble__" section (created by the lexer for lines that
        // appear before any section header) also falls through here intentionally:
        // it is an implementation artefact, not a user-visible section, and its
        // content is silently discarded (matching C++ BasicConfig behavior for the
        // default-section "" lines that the consumer never looks up).
        //
        // All other unknown sections are silently dropped (lenient INI per design §5.3).
        _ => {}
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Category-3 handwritten adapters
// ---------------------------------------------------------------------------

/// Adapt the `[server]` section.
/// Bare lines are port names; kv pairs are `PortDefaults`.
fn adapt_server(sec: &RawSection, p: &mut Parsed) -> Result<(), ConfigError> {
    let mut port_names = Vec::new();

    for line in &sec.lines {
        match &line.kind {
            RawLineKind::BareValue(v) => {
                let name = v.trim().to_owned();
                if !name.is_empty() {
                    port_names.push(name);
                }
            }
            RawLineKind::KeyValue { .. } => {}
        }
    }

    // kv pairs form the PortDefaults. Use adapt_port_section to avoid the
    // flatten/deserialize_any issue with Vec<IpNet> and Vec<PortProtocol>.
    let defaults_pc = adapt_port_section(sec)?;

    p.server = ServerConfig { port_names, defaults: defaults_pc.effective };
    Ok(())
}

/// Adapt the `[crawl]` section.
/// Single bare boolean → `LegacyBool`; kv pairs → `Detailed`.
fn adapt_crawl(sec: &RawSection) -> Result<CrawlConfig, ConfigError> {
    // Check if there's any kv content.
    let has_kv = sec.lines.iter().any(|l| matches!(l.kind, RawLineKind::KeyValue { .. }));

    if has_kv {
        // Detailed form: deserialize kv pairs.
        let map = sec.lookup();
        let overlay = map.get("overlay").map(|v| parse_ini_bool(v)).transpose()?.unwrap_or(false);
        let server = map.get("server").map(|v| parse_ini_bool(v)).transpose()?.unwrap_or(false);
        let counts = map.get("counts").map(|v| parse_ini_bool(v)).transpose()?.unwrap_or(false);
        let unl = map.get("unl").map(|v| parse_ini_bool(v)).transpose()?.unwrap_or(false);
        return Ok(CrawlConfig::Detailed { overlay, server, counts, unl });
    }

    // Check for a bare-value bool.
    let bare_values: Vec<&str> = sec
        .lines
        .iter()
        .filter_map(|l| if let RawLineKind::BareValue(v) = &l.kind { Some(v.as_str()) } else { None })
        .collect();

    if let Some(&first) = bare_values.first() {
        let b = parse_ini_bool(first)?;
        return Ok(CrawlConfig::LegacyBool(b));
    }

    // Empty section — return default.
    Ok(CrawlConfig::default())
}

/// Get the single content line of a bare-line section.
/// Works for both `BareValue` lines and `KeyValue` lines that represent a single entry.
fn adapt_single_line(sec: &RawSection) -> Result<String, ConfigError> {
    // Look for bare value lines first.
    for line in &sec.lines {
        if let RawLineKind::BareValue(v) = &line.kind {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_owned());
            }
        }
    }
    // Fall back to kv lines (single-value sections that happen to contain `=`).
    for line in &sec.lines {
        if let RawLineKind::KeyValue { key, value } = &line.kind {
            // Return `key=value` as the full string? Actually single-line sections
            // shouldn't be kv. Return error if nothing found.
            let _ = (key, value);
        }
    }
    Err(ConfigError::grammar(
        "single_line",
        "",
        format!("section [{}]: expected exactly one value line, found none", sec.name),
    ))
}

/// Concatenate all bare-value lines (and key-value lines) into a single blob string.
/// Used for `[validation_seed]`, `[validator_token]`, `[validator_key_revocation]`.
fn adapt_multi_line_blob(sec: &RawSection) -> Result<String, ConfigError> {
    let mut parts = Vec::new();
    for line in &sec.lines {
        match &line.kind {
            RawLineKind::BareValue(v) => parts.push(v.trim().to_owned()),
            RawLineKind::KeyValue { key, value } => {
                parts.push(format!("{}={}", key, value));
            }
        }
    }
    Ok(parts.join(""))
}

/// Parse `[network_id]`: `main` → 0, `testnet` → 1, `devnet` → 2, or decimal integer.
fn adapt_network_id(sec: &RawSection) -> Result<u32, ConfigError> {
    let s = adapt_single_line(sec)?;
    match s.trim().to_ascii_lowercase().as_str() {
        "main" | "mainnet" => Ok(0),
        "testnet" => Ok(1),
        "devnet" => Ok(2),
        other => parse_ini_int(other),
    }
}

/// Parse `[node_size]`: tiny/small/medium/large/huge.
fn adapt_node_size(sec: &RawSection) -> Result<NodeSize, ConfigError> {
    let s = adapt_single_line(sec)?;
    match s.trim().to_ascii_lowercase().as_str() {
        "tiny" => Ok(NodeSize::Tiny),
        "small" => Ok(NodeSize::Small),
        "medium" => Ok(NodeSize::Medium),
        "large" => Ok(NodeSize::Large),
        "huge" => Ok(NodeSize::Huge),
        other => Err(ConfigError::grammar(
            "node_size",
            other,
            "expected tiny, small, medium, large, or huge",
        )),
    }
}

/// Parse `[ledger_history]`: `full` → Full, `none` → None_, or decimal integer.
fn adapt_ledger_history(sec: &RawSection) -> Result<LedgerHistory, ConfigError> {
    let s = adapt_single_line(sec)?;
    match s.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(LedgerHistory::Full),
        "none" => Ok(LedgerHistory::None_),
        other => {
            let n: u32 = parse_ini_int(other)?;
            Ok(LedgerHistory::Count(n))
        }
    }
}

/// Parse `[fetch_depth]`: `full` → Full, `none` → None_, or decimal integer.
/// Per analysis §5, floor is 10 (lenient clamp).
fn adapt_fetch_depth(sec: &RawSection) -> Result<FetchDepth, ConfigError> {
    let s = adapt_single_line(sec)?;
    match s.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(FetchDepth::Full),
        "none" => Ok(FetchDepth::None_),
        other => {
            let n: u32 = parse_ini_int(other)?;
            // Floor at 10 (lenient clamp per analysis §5).
            let n = n.max(10);
            Ok(FetchDepth::Count(n))
        }
    }
}

/// Parse `[relay_validations]` / `[relay_proposals]`.
fn adapt_relay_policy(sec: &RawSection) -> Result<RelayPolicy, ConfigError> {
    let s = adapt_single_line(sec)?;
    match s.trim().to_ascii_lowercase().as_str() {
        "all" => Ok(RelayPolicy::All),
        "trusted" => Ok(RelayPolicy::Trusted),
        "drop_untrusted" | "dropuntrusted" => Ok(RelayPolicy::DropUntrusted),
        other => Err(ConfigError::grammar(
            "relay_policy",
            other,
            "expected all, trusted, or drop_untrusted",
        )),
    }
}

/// Adapt bare-line lines for trusted validators (`[validators]` / `[validator_keys]`).
/// Grammar: `<base58_key> [optional_label]`.
fn adapt_trusted_validators(sec: &RawSection) -> Result<Vec<TrustedValidator>, ConfigError> {
    let mut result = Vec::new();
    for line in &sec.lines {
        if let RawLineKind::BareValue(v) = &line.kind {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            let (key, label) = split_key_label(v);
            result.push(TrustedValidator {
                key: key.to_owned(),
                label: label.map(|l| l.to_owned()),
            });
        }
    }
    Ok(result)
}

/// Adapt bare-line lines for `[cluster_nodes]`.
/// Grammar: `<base58_key> [optional_label]`.
fn adapt_cluster_nodes(sec: &RawSection) -> Result<Vec<ClusterNode>, ConfigError> {
    let mut result = Vec::new();
    for line in &sec.lines {
        if let RawLineKind::BareValue(v) = &line.kind {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            let (key, label) = split_key_label(v);
            result.push(ClusterNode {
                key: key.to_owned(),
                label: label.map(|l| l.to_owned()),
            });
        }
    }
    Ok(result)
}

/// Split `<key> [label]` into `(key, Option<label>)`.
fn split_key_label(s: &str) -> (&str, Option<&str>) {
    if let Some(space) = s.find(char::is_whitespace) {
        let key = &s[..space];
        let label = s[space..].trim();
        if label.is_empty() {
            (key, None)
        } else {
            (key, Some(label))
        }
    } else {
        (s, None)
    }
}

/// Adapt bare-line lines for `[amendments]` / `[veto_amendments]`.
/// Grammar: `<64-hex> <name>`.
fn adapt_known_amendments(sec: &RawSection) -> Result<Vec<KnownAmendment>, ConfigError> {
    let mut result = Vec::new();
    for line in &sec.lines {
        if let RawLineKind::BareValue(v) = &line.kind {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            let (hex_part, name_part) = if let Some(space) = v.find(char::is_whitespace) {
                (&v[..space], v[space..].trim())
            } else {
                return Err(ConfigError::grammar(
                    "amendment",
                    v,
                    "expected `<64-hex> <name>`",
                ));
            };

            if hex_part.len() != 64 {
                return Err(ConfigError::grammar(
                    "amendment",
                    v,
                    "amendment ID must be exactly 64 hex characters",
                ));
            }
            let mut id = [0u8; 32];
            hex::decode_to_slice(hex_part, &mut id).map_err(|_| {
                ConfigError::grammar("amendment", v, "invalid hex in amendment ID")
            })?;

            result.push(KnownAmendment {
                id,
                name: name_part.to_owned(),
            });
        }
    }
    Ok(result)
}

/// Adapt `[rpc_startup]` — each line is a raw JSON value.
fn adapt_rpc_startup(sec: &RawSection) -> Result<Vec<serde_json::Value>, ConfigError> {
    let mut result = Vec::new();
    for line in &sec.lines {
        let s = match &line.kind {
            RawLineKind::BareValue(v) => v.trim().to_owned(),
            RawLineKind::KeyValue { key, value } => format!("{{\"{}\":{}}}", key, value),
        };
        if s.is_empty() {
            continue;
        }
        let val: serde_json::Value = serde_json::from_str(&s).map_err(|e| {
            ConfigError::grammar("rpc_startup", &s, format!("invalid JSON: {e}"))
        })?;
        result.push(val);
    }
    Ok(result)
}

/// Adapt `[node_db]` / `[import_db]`.
/// Known keys are deserialized into `NodeDbConfig`; unknown keys go into `backend_extras`.
///
/// Bare-value lines (lines that are not `key=value`) inside `[node_db]` are silently dropped —
/// this matches C++ lenient INI behavior where non-kv content in a kv section is simply ignored.
fn adapt_node_db(sec: &RawSection) -> Result<NodeDbConfig, ConfigError> {
    let map = sec.lookup();
    let mut cfg = NodeDbConfig::default();
    let mut extras = BTreeMap::new();

    for (key, value) in &map {
        match *key {
            "type" => {
                cfg.kind = match value.to_ascii_lowercase().as_str() {
                    "nudb" => NodeDbKind::NuDb,
                    "rocksdb" => NodeDbKind::RocksDb,
                    other => return Err(ConfigError::grammar("node_db.type", other, "expected nudb or rocksdb")),
                };
            }
            "path" => {
                cfg.path = PathBuf::from(value);
            }
            "fast_load" => {
                cfg.fast_load = parse_ini_bool(value)?;
            }
            "earliest_seq" => {
                cfg.earliest_seq = parse_ini_int(value)?;
            }
            "online_delete" => {
                cfg.online_delete = Some(parse_ini_int(value)?);
            }
            "advisory_delete" => {
                cfg.advisory_delete = parse_ini_bool(value)?;
            }
            "delete_batch" => {
                cfg.delete_batch = parse_ini_int(value)?;
            }
            "back_off_milliseconds" => {
                cfg.back_off_milliseconds = parse_ini_int(value)?;
            }
            "age_threshold_seconds" => {
                cfg.age_threshold_seconds = parse_ini_int(value)?;
            }
            "recovery_wait_seconds" => {
                cfg.recovery_wait_seconds = parse_ini_int(value)?;
            }
            "nudb_block_size" => {
                cfg.nudb_block_size = parse_ini_int(value)?;
            }
            other => {
                // Unknown key: put in backend_extras.
                extras.insert(other.to_owned(), (*value).to_owned());
            }
        }
    }

    cfg.backend_extras = extras;
    Ok(cfg)
}

/// Adapt `[sqlite]` — the mutual-exclusion between `safety_level` and tuning triple.
fn adapt_sqlite(sec: &RawSection) -> Result<crate::types::sqlite::SqliteConfig, ConfigError> {
    use crate::types::sqlite::*;

    let map = sec.lookup();

    let has_safety = map.contains_key("safety_level");
    let has_tuning = map.contains_key("journal_mode")
        || map.contains_key("synchronous")
        || map.contains_key("temp_store");

    if has_safety && has_tuning {
        return Err(ConfigError::mutual_exclusion(
            "safety_level",
            "journal_mode/synchronous/temp_store",
        ));
    }

    let journal_size_limit = map
        .get("journal_size_limit")
        .map(|v| parse_ini_int::<i64, _>(v))
        .transpose()?
        .unwrap_or(1_582_080);

    let mode = if has_safety {
        let level_str = map["safety_level"].to_ascii_lowercase();
        let level = match level_str.as_str() {
            "high" => SqliteSafety::High,
            "low" => SqliteSafety::Low,
            other => return Err(ConfigError::grammar("sqlite.safety_level", other, "expected high or low")),
        };
        SqliteMode::Safety { level }
    } else if has_tuning {
        let journal_mode = map.get("journal_mode").map(|v| {
            match v.to_ascii_lowercase().as_str() {
                "delete" => Ok(SqliteJournalMode::Delete),
                "truncate" => Ok(SqliteJournalMode::Truncate),
                "persist" => Ok(SqliteJournalMode::Persist),
                "memory" => Ok(SqliteJournalMode::Memory),
                "wal" => Ok(SqliteJournalMode::Wal),
                "off" => Ok(SqliteJournalMode::Off),
                other => Err(ConfigError::grammar("sqlite.journal_mode", other, "invalid journal mode")),
            }
        }).transpose()?;

        let synchronous = map.get("synchronous").map(|v| {
            match v.to_ascii_lowercase().as_str() {
                "off" => Ok(SqliteSynchronous::Off),
                "normal" => Ok(SqliteSynchronous::Normal),
                "full" => Ok(SqliteSynchronous::Full),
                "extra" => Ok(SqliteSynchronous::Extra),
                other => Err(ConfigError::grammar("sqlite.synchronous", other, "invalid synchronous value")),
            }
        }).transpose()?;

        let temp_store = map.get("temp_store").map(|v| {
            match v.to_ascii_lowercase().as_str() {
                "default" => Ok(SqliteTempStore::Default),
                "file" => Ok(SqliteTempStore::File),
                "memory" => Ok(SqliteTempStore::Memory),
                other => Err(ConfigError::grammar("sqlite.temp_store", other, "invalid temp_store value")),
            }
        }).transpose()?;

        let page_size = map
            .get("page_size")
            .map(|v| parse_ini_int::<u32, _>(v))
            .transpose()?
            .unwrap_or(4096);

        SqliteMode::Tuning { journal_mode, synchronous, temp_store, page_size }
    } else {
        SqliteMode::Default
    };

    Ok(crate::types::sqlite::SqliteConfig { mode, journal_size_limit })
}

/// Adapt `[insight]` — handwritten because `InsightServer` and `SocketAddr` need custom handling.
fn adapt_insight(sec: &RawSection) -> Result<InsightConfig, ConfigError> {
    let map = sec.lookup();

    let server = match map.get("server").map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("statsd") | None => InsightServer::StatsD,
        Some(other) => return Err(ConfigError::grammar("insight.server", other, "expected statsd")),
    };

    let address = map
        .get("address")
        .map(|v| v.parse().map_err(|_| ConfigError::grammar("insight.address", *v, "invalid socket address")))
        .transpose()?;

    let prefix = map.get("prefix").map(|v| (*v).to_owned());

    Ok(InsightConfig { server, address, prefix })
}

// ---------------------------------------------------------------------------
// Lenient-validation hooks on types
// ---------------------------------------------------------------------------

impl OverlayConfig {
    pub(crate) fn validate_lenient(&mut self) {
        self.max_unknown_time = self.max_unknown_time.clamp(300, 1800);
        self.max_diverged_time = self.max_diverged_time.clamp(60, 900);
    }
}

impl ReduceRelayConfig {
    /// Apply silent INI clamps per analysis §5:
    /// - `vp_base_squelch_max_selected_peers` ≥ 3
    /// - `tx_min_peers` ≥ 10
    /// - `tx_relay_percentage` in 10..=100
    pub(crate) fn validate_lenient(&mut self) {
        self.vp_base_squelch_max_selected_peers = self.vp_base_squelch_max_selected_peers.max(3);
        self.tx_min_peers = self.tx_min_peers.max(10);
        self.tx_relay_percentage = self.tx_relay_percentage.clamp(10, 100);
    }
}

impl NodeDbConfig {
    /// Apply silent INI clamps per analysis §5:
    /// - `earliest_seq` ≥ 1
    /// - `online_delete` ≥ 256 when set
    /// - `nudb_block_size` clamped to power-of-2 in 4096..=32768
    pub(crate) fn validate_lenient(&mut self) {
        if self.earliest_seq < 1 {
            self.earliest_seq = 1;
        }
        if let Some(od) = self.online_delete {
            if od < 256 {
                self.online_delete = Some(256);
            }
        }
        // nudb_block_size must be a power of 2 in [4096, 32768].
        // If out of range or not a power of 2, clamp to the nearest valid value.
        let bs = self.nudb_block_size;
        if bs != 0 && !(4096..=32768).contains(&bs) || (bs != 0 && !bs.is_power_of_two()) {
            // Round down to previous power of 2 within range.
            let clamped = bs.clamp(4096, 32768);
            self.nudb_block_size = if clamped.is_power_of_two() {
                clamped
            } else {
                // Previous power of 2.
                let prev = 1u32 << (u32::BITS - clamped.leading_zeros() - 1);
                prev.clamp(4096, 32768)
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lexer::tokenize;

    fn parse(ini: &str) -> Config {
        let raw = tokenize(ini).expect("tokenize failed");
        adapt(raw).expect("adapt failed")
    }

    #[test]
    fn overlay_section() {
        let cfg = parse("[overlay]\nmax_unknown_time=600\nmax_diverged_time=300\n");
        assert_eq!(cfg.overlay().max_unknown_time, 600);
        assert_eq!(cfg.overlay().max_diverged_time, 300);
    }

    #[test]
    fn overlay_clamped() {
        // Values outside range should be clamped.
        let cfg = parse("[overlay]\nmax_unknown_time=9999\nmax_diverged_time=5\n");
        assert_eq!(cfg.overlay().max_unknown_time, 1800);
        assert_eq!(cfg.overlay().max_diverged_time, 60);
    }

    #[test]
    fn ips_section() {
        let cfg = parse("[ips]\nr.ripple.com 51235\n");
        assert_eq!(cfg.ips().len(), 1);
        assert!(matches!(&cfg.ips()[0].host, HostKind::Hostname(h) if h == "r.ripple.com"));
    }

    #[test]
    fn network_id_named() {
        let cfg = parse("[network_id]\nmain\n");
        assert_eq!(cfg.network_id(), 0);
        let cfg = parse("[network_id]\ntestnet\n");
        assert_eq!(cfg.network_id(), 1);
    }

    #[test]
    fn network_id_numeric() {
        let cfg = parse("[network_id]\n1234\n");
        assert_eq!(cfg.network_id(), 1234);
    }

    #[test]
    fn ledger_history_full() {
        let cfg = parse("[ledger_history]\nfull\n");
        assert_eq!(cfg.parsed.ledger_history, LedgerHistory::Full);
    }

    #[test]
    fn ledger_history_count() {
        let cfg = parse("[ledger_history]\n512\n");
        assert_eq!(cfg.parsed.ledger_history, LedgerHistory::Count(512));
    }

    #[test]
    fn crawl_legacy_bool() {
        let cfg = parse("[crawl]\ntrue\n");
        assert_eq!(cfg.crawl(), &CrawlConfig::LegacyBool(true));
    }

    #[test]
    fn crawl_detailed() {
        let cfg = parse("[crawl]\noverlay=1\nserver=0\ncounts=1\nunl=0\n");
        assert_eq!(
            cfg.crawl(),
            &CrawlConfig::Detailed { overlay: true, server: false, counts: true, unl: false }
        );
    }

    #[test]
    fn validator_token_blob() {
        let cfg = parse("[validator_token]\nABCDEF\n1234\n");
        assert_eq!(cfg.parsed.validator_token.as_deref(), Some("ABCDEF1234"));
    }

    #[test]
    fn trusted_validators() {
        let cfg = parse("[validators]\nnBvNd7Y5RvRoaFHkHQyQbT7PVczeTrFJQe Alice\nnBvXXX\n");
        assert_eq!(cfg.trusted_validators().len(), 2);
        assert_eq!(cfg.trusted_validators()[0].label.as_deref(), Some("Alice"));
        assert_eq!(cfg.trusted_validators()[1].label, None);
    }

    #[test]
    fn vl_config() {
        let cfg = parse("[vl]\nenabled=1\n");
        assert!(cfg.parsed.vl.enabled);
    }

    #[test]
    fn relay_policy() {
        let cfg = parse("[relay_validations]\nall\n");
        assert_eq!(cfg.parsed.relay_untrusted_validations, RelayPolicy::All);
    }

    #[test]
    fn unknown_section_silently_dropped() {
        // Should not error.
        let _cfg = parse("[totally_unknown_section]\nfoo=bar\n");
    }

    #[test]
    fn max_transactions_clamped() {
        // Below 100: clamped to 100.
        let cfg = parse("[max_transactions]\n5\n");
        assert_eq!(cfg.max_transactions(), 100);
        // Above 1000: clamped to 1000.
        let cfg = parse("[max_transactions]\n99999\n");
        assert_eq!(cfg.max_transactions(), 1000);
    }

    #[test]
    fn peer_private_bool() {
        let cfg = parse("[peer_private]\n1\n");
        assert!(cfg.peer_private());
    }

    #[test]
    fn node_size_parsed() {
        let cfg = parse("[node_size]\nhuge\n");
        assert_eq!(cfg.parsed.node_size, Some(NodeSize::Huge));
    }

    // ---- NEW TESTS: [server] section ----

    #[test]
    fn server_bare_lines_become_port_names() {
        let cfg = parse("[server]\nport_rpc_admin_local\nport_peer\n");
        assert_eq!(cfg.server().port_names, vec!["port_rpc_admin_local", "port_peer"]);
    }

    #[test]
    fn server_empty_section_gives_empty_port_names() {
        let cfg = parse("[server]\n");
        assert!(cfg.server().port_names.is_empty());
    }

    #[test]
    fn server_kv_lines_go_into_port_defaults() {
        // kv pairs in [server] become PortDefaults
        let cfg = parse("[server]\nport_rpc\nsend_queue_limit=50\n");
        assert_eq!(cfg.server().port_names, vec!["port_rpc"]);
        assert_eq!(cfg.server().defaults.send_queue_limit, 50);
    }

    #[test]
    fn server_port_section_parsed() {
        let ini = "[server]\nport_rpc\n[port_rpc]\nport=5005\n";
        let cfg = parse(ini);
        assert!(cfg.port("port_rpc").is_some());
        assert_eq!(cfg.port("port_rpc").unwrap().port, 5005);
    }

    #[test]
    fn port_not_in_server_list_silently_dropped() {
        // [port_stray] listed but not in [server]'s bare lines
        let ini = "[server]\nport_rpc\n[port_rpc]\nport=5005\n[port_stray]\nport=9999\n";
        let cfg = parse(ini);
        assert!(cfg.port("port_rpc").is_some());
        assert!(cfg.port("port_stray").is_none()); // not in server's port_names
    }

    // ---- NEW TESTS: [crawl] ----

    #[test]
    fn crawl_legacy_false() {
        let cfg = parse("[crawl]\nfalse\n");
        assert_eq!(cfg.crawl(), &CrawlConfig::LegacyBool(false));
    }

    #[test]
    fn crawl_legacy_0() {
        let cfg = parse("[crawl]\n0\n");
        assert_eq!(cfg.crawl(), &CrawlConfig::LegacyBool(false));
    }

    #[test]
    fn crawl_legacy_1() {
        let cfg = parse("[crawl]\n1\n");
        assert_eq!(cfg.crawl(), &CrawlConfig::LegacyBool(true));
    }

    #[test]
    fn crawl_detailed_all_fields() {
        let cfg = parse("[crawl]\noverlay=1\nserver=1\ncounts=1\nunl=1\n");
        assert_eq!(
            cfg.crawl(),
            &CrawlConfig::Detailed { overlay: true, server: true, counts: true, unl: true }
        );
    }

    #[test]
    fn crawl_detailed_partial_fields_default_to_false() {
        let cfg = parse("[crawl]\noverlay=1\n");
        assert_eq!(
            cfg.crawl(),
            &CrawlConfig::Detailed { overlay: true, server: false, counts: false, unl: false }
        );
    }

    #[test]
    fn crawl_empty_returns_default() {
        let cfg = parse("[crawl]\n");
        // Empty section → Detailed with all false (default)
        assert_eq!(cfg.crawl(), &CrawlConfig::default());
    }

    // ---- NEW TESTS: [network_id] ----

    #[test]
    fn network_id_main_is_0() {
        let cfg = parse("[network_id]\nmain\n");
        assert_eq!(cfg.network_id(), 0);
    }

    #[test]
    fn network_id_mainnet_is_0() {
        let cfg = parse("[network_id]\nmainnet\n");
        assert_eq!(cfg.network_id(), 0);
    }

    #[test]
    fn network_id_testnet_is_1() {
        let cfg = parse("[network_id]\ntestnet\n");
        assert_eq!(cfg.network_id(), 1);
    }

    #[test]
    fn network_id_devnet_is_2() {
        let cfg = parse("[network_id]\ndevnet\n");
        assert_eq!(cfg.network_id(), 2);
    }

    #[test]
    fn network_id_numeric_42() {
        let cfg = parse("[network_id]\n42\n");
        assert_eq!(cfg.network_id(), 42);
    }

    #[test]
    fn network_id_case_insensitive() {
        let cfg = parse("[network_id]\nMAIN\n");
        assert_eq!(cfg.network_id(), 0);
        let cfg2 = parse("[network_id]\nTESTNET\n");
        assert_eq!(cfg2.network_id(), 1);
    }

    // ---- NEW TESTS: [ledger_history] ----

    #[test]
    fn ledger_history_none() {
        let cfg = parse("[ledger_history]\nnone\n");
        assert_eq!(cfg.parsed.ledger_history, LedgerHistory::None_);
    }

    #[test]
    fn ledger_history_numeric_256() {
        let cfg = parse("[ledger_history]\n256\n");
        assert_eq!(cfg.parsed.ledger_history, LedgerHistory::Count(256));
    }

    #[test]
    fn ledger_history_case_insensitive() {
        let cfg = parse("[ledger_history]\nFULL\n");
        assert_eq!(cfg.parsed.ledger_history, LedgerHistory::Full);
    }

    // ---- NEW TESTS: [fetch_depth] ----

    #[test]
    fn fetch_depth_full() {
        let cfg = parse("[fetch_depth]\nfull\n");
        assert_eq!(cfg.parsed.fetch_depth, FetchDepth::Full);
    }

    #[test]
    fn fetch_depth_none() {
        let cfg = parse("[fetch_depth]\nnone\n");
        assert_eq!(cfg.parsed.fetch_depth, FetchDepth::None_);
    }

    #[test]
    fn fetch_depth_numeric() {
        let cfg = parse("[fetch_depth]\n500\n");
        assert_eq!(cfg.parsed.fetch_depth, FetchDepth::Count(500));
    }

    #[test]
    fn fetch_depth_clamps_below_10() {
        // Values < 10 are floored to 10 (lenient clamp per analysis §5)
        let cfg = parse("[fetch_depth]\n5\n");
        assert_eq!(cfg.parsed.fetch_depth, FetchDepth::Count(10));
    }

    #[test]
    fn fetch_depth_1_clamped_to_10() {
        let cfg = parse("[fetch_depth]\n1\n");
        assert_eq!(cfg.parsed.fetch_depth, FetchDepth::Count(10));
    }

    // ---- NEW TESTS: [relay_validations] / [relay_proposals] ----

    #[test]
    fn relay_validations_all() {
        let cfg = parse("[relay_validations]\nall\n");
        assert_eq!(cfg.parsed.relay_untrusted_validations, RelayPolicy::All);
    }

    #[test]
    fn relay_validations_trusted() {
        let cfg = parse("[relay_validations]\ntrusted\n");
        assert_eq!(cfg.parsed.relay_untrusted_validations, RelayPolicy::Trusted);
    }

    #[test]
    fn relay_validations_drop_untrusted() {
        let cfg = parse("[relay_validations]\ndrop_untrusted\n");
        assert_eq!(cfg.parsed.relay_untrusted_validations, RelayPolicy::DropUntrusted);
    }

    #[test]
    fn relay_proposals_trusted() {
        let cfg = parse("[relay_proposals]\ntrusted\n");
        assert_eq!(cfg.parsed.relay_untrusted_proposals, RelayPolicy::Trusted);
    }

    // ---- NEW TESTS: [validators] and [validator_keys] ----

    #[test]
    fn validator_keys_section_feeds_trusted_validators() {
        let cfg = parse("[validator_keys]\nnBvNd7Y5RvRoaFHkHQyQbT7PVczeTrFJQe Alice\nnBvXXX\n");
        assert_eq!(cfg.trusted_validators().len(), 2);
        assert_eq!(cfg.trusted_validators()[0].key, "nBvNd7Y5RvRoaFHkHQyQbT7PVczeTrFJQe");
        assert_eq!(cfg.trusted_validators()[0].label.as_deref(), Some("Alice"));
        assert_eq!(cfg.trusted_validators()[1].key, "nBvXXX");
        assert_eq!(cfg.trusted_validators()[1].label, None);
    }

    #[test]
    fn validators_and_validator_keys_both_feed_trusted() {
        let ini = "[validators]\nKEY_A\n[validator_keys]\nKEY_B\n";
        let cfg = parse(ini);
        assert_eq!(cfg.trusted_validators().len(), 2);
        let keys: Vec<_> = cfg.trusted_validators().iter().map(|v| v.key.as_str()).collect();
        assert!(keys.contains(&"KEY_A"));
        assert!(keys.contains(&"KEY_B"));
    }

    #[test]
    fn trusted_validator_with_label() {
        let cfg = parse("[validators]\nnBvNd7Y5 My Validator Node\n");
        let v = &cfg.trusted_validators()[0];
        assert_eq!(v.key, "nBvNd7Y5");
        assert_eq!(v.label.as_deref(), Some("My Validator Node"));
    }

    // ---- NEW TESTS: [max_transactions] ----

    #[test]
    fn max_transactions_below_min_clamped_to_100() {
        let cfg = parse("[max_transactions]\n50\n");
        assert_eq!(cfg.max_transactions(), 100);
    }

    #[test]
    fn max_transactions_above_max_clamped_to_1000() {
        let cfg = parse("[max_transactions]\n9999\n");
        assert_eq!(cfg.max_transactions(), 1000);
    }

    #[test]
    fn max_transactions_exact_min() {
        let cfg = parse("[max_transactions]\n100\n");
        assert_eq!(cfg.max_transactions(), 100);
    }

    #[test]
    fn max_transactions_exact_max() {
        let cfg = parse("[max_transactions]\n1000\n");
        assert_eq!(cfg.max_transactions(), 1000);
    }

    #[test]
    fn max_transactions_in_range() {
        let cfg = parse("[max_transactions]\n500\n");
        assert_eq!(cfg.max_transactions(), 500);
    }

    // ---- NEW TESTS: [validator_token] multi-line blob ----

    #[test]
    fn validator_token_three_lines_concatenated() {
        let cfg = parse("[validator_token]\nline1\nline2\nline3\n");
        assert_eq!(cfg.parsed.validator_token.as_deref(), Some("line1line2line3"));
    }

    #[test]
    fn validator_token_single_line() {
        let cfg = parse("[validator_token]\nABCDEF1234\n");
        assert_eq!(cfg.parsed.validator_token.as_deref(), Some("ABCDEF1234"));
    }

    #[test]
    fn validation_seed_blob() {
        let cfg = parse("[validation_seed]\nABC\nDEF\n");
        assert_eq!(cfg.parsed.validation_seed.as_deref(), Some("ABCDEF"));
    }

    #[test]
    fn validator_key_revocation_blob() {
        let cfg = parse("[validator_key_revocation]\nXXX\nYYY\n");
        assert_eq!(cfg.parsed.validator_key_revocation.as_deref(), Some("XXXYYY"));
    }

    // ---- NEW TESTS: unknown sections silently dropped ----

    #[test]
    fn unknown_section_does_not_affect_other_sections() {
        let cfg = parse("[totally_unknown]\nfoo=bar\n[network_id]\n42\n");
        assert_eq!(cfg.network_id(), 42);
    }

    #[test]
    fn multiple_unknown_sections_all_dropped() {
        let _cfg = parse("[unknown1]\nfoo=bar\n[unknown2]\nbaz=qux\n[unknown3]\ntest=value\n");
        // No panic = success
    }

    // ---- NEW TESTS: single-line sections ----

    #[test]
    fn database_path_parsed() {
        let cfg = parse("[database_path]\n/var/lib/rippled/db\n");
        assert!(cfg.parsed.database_path.is_some());
    }

    #[test]
    fn debug_logfile_parsed() {
        let cfg = parse("[debug_logfile]\n/var/log/rippled/debug.log\n");
        assert!(cfg.parsed.debug_logfile.is_some());
    }

    #[test]
    fn server_domain_parsed() {
        let cfg = parse("[server_domain]\nexample.com\n");
        assert_eq!(cfg.server_domain(), Some("example.com"));
    }

    #[test]
    fn compression_bool_section() {
        let cfg = parse("[compression]\n1\n");
        assert!(cfg.compression());
        let cfg2 = parse("[compression]\n0\n");
        assert!(!cfg2.compression());
    }

    #[test]
    fn ledger_replay_section() {
        let cfg = parse("[ledger_replay]\ntrue\n");
        assert!(cfg.ledger_replay());
    }

    #[test]
    fn beta_rpc_api_section() {
        let cfg = parse("[beta_rpc_api]\n1\n");
        assert!(cfg.beta_rpc_api());
    }

    #[test]
    fn peer_private_false() {
        let cfg = parse("[peer_private]\n0\n");
        assert!(!cfg.peer_private());
    }

    #[test]
    fn peers_max_section() {
        let cfg = parse("[peers_max]\n100\n");
        assert_eq!(cfg.peers_max(), 100);
    }

    #[test]
    fn workers_section() {
        let cfg = parse("[workers]\n4\n");
        assert_eq!(cfg.workers(), 4);
    }

    #[test]
    fn node_size_all_values() {
        for (s, expected) in &[
            ("tiny", NodeSize::Tiny),
            ("small", NodeSize::Small),
            ("medium", NodeSize::Medium),
            ("large", NodeSize::Large),
            ("huge", NodeSize::Huge),
        ] {
            let cfg = parse(&format!("[node_size]\n{}\n", s));
            assert_eq!(cfg.parsed.node_size, Some(*expected));
        }
    }

    // ---- NEW TESTS: overlay lenient clamping ----

    #[test]
    fn overlay_max_unknown_time_min_clamped() {
        let cfg = parse("[overlay]\nmax_unknown_time=100\n");
        // Min is 300, so 100 should be clamped to 300
        assert_eq!(cfg.overlay().max_unknown_time, 300);
    }

    #[test]
    fn overlay_max_diverged_time_min_clamped() {
        let cfg = parse("[overlay]\nmax_diverged_time=10\n");
        // Min is 60, so 10 should be clamped to 60
        assert_eq!(cfg.overlay().max_diverged_time, 60);
    }

    #[test]
    fn overlay_max_diverged_time_max_clamped() {
        let cfg = parse("[overlay]\nmax_diverged_time=9999\n");
        // Max is 900
        assert_eq!(cfg.overlay().max_diverged_time, 900);
    }

    // ---- NEW TESTS: [ips] and [ips_fixed] ----

    #[test]
    fn ips_empty_section() {
        let cfg = parse("[ips]\n");
        assert!(cfg.ips().is_empty());
    }

    #[test]
    fn ips_multiple_entries() {
        let cfg = parse("[ips]\nr.ripple.com 51235\naltnet.ripple.com 51235\n");
        assert_eq!(cfg.ips().len(), 2);
    }

    #[test]
    fn ips_fixed_section() {
        let cfg = parse("[ips_fixed]\nr.ripple.com 51235\n");
        assert_eq!(cfg.ips_fixed().len(), 1);
    }

    // ---- NEW TESTS: [features] ----

    #[test]
    fn features_section() {
        let cfg = parse("[features]\nCryptoConditions\nMultiSign\n");
        assert!(cfg.features().contains("CryptoConditions"));
        assert!(cfg.features().contains("MultiSign"));
    }

    // ---- NEW TESTS: [sntp_servers] ----

    #[test]
    fn sntp_servers_section() {
        let cfg = parse("[sntp_servers]\ntime.windows.com\npool.ntp.org\n");
        assert_eq!(cfg.sntp_servers().len(), 2);
        assert!(cfg.sntp_servers().contains(&"time.windows.com".to_owned()));
    }

    // ---- NEW TESTS: [validator_list_sites] / [validator_list_keys] ----

    #[test]
    fn validator_list_sites_section() {
        let cfg = parse("[validator_list_sites]\nhttps://vl.ripple.com\n");
        assert_eq!(cfg.validator_list_sites().len(), 1);
        assert_eq!(cfg.validator_list_sites()[0], "https://vl.ripple.com");
    }

    #[test]
    fn validator_list_keys_section() {
        let cfg = parse("[validator_list_keys]\nED2677ABFFD1B33AC6FBC3062B71F1E8397C1505E1C42C064D11827\n");
        assert_eq!(cfg.validator_list_keys().len(), 1);
    }

    // ---- NEW TESTS: [websocket_ping_frequency] ----

    #[test]
    fn websocket_ping_frequency_section() {
        let cfg = parse("[websocket_ping_frequency]\n30\n");
        assert_eq!(cfg.websocket_ping_frequency(), Some(30));
    }

    // ---- NEW TESTS: missing sections use defaults ----

    #[test]
    fn defaults_when_no_sections_present() {
        let cfg = parse("");
        assert_eq!(cfg.network_id(), 0); // default
        assert_eq!(cfg.max_transactions(), 250); // default
        assert!(!cfg.peer_private()); // default false
        assert_eq!(cfg.peers_max(), 0); // default
    }

    // ---- F19: unknown feature names survive parse (Phase-3 invariant) ----

    /// Contract: `[features]` entries are raw strings; unknown names survive parse
    /// so the downstream C++ consumer can apply validation against the registered list.
    #[test]
    fn unknown_feature_name_survives_parse() {
        let cfg = parse("[features]\nDefinitelyNotARealAmendment\nAlsoFake_2099\n");
        assert!(cfg.features().contains("DefinitelyNotARealAmendment"),
            "unknown feature name must survive parse (Phase-3 validation deferred to C++ consumer)");
        assert!(cfg.features().contains("AlsoFake_2099"));
    }

    // ---- F27: [node_db] unknown key goes to backend_extras ----

    #[test]
    fn node_db_unknown_key_goes_to_extras() {
        // "earliestseq" (missing underscore) is not a known key; goes to backend_extras.
        let cfg = parse("[node_db]\ntype=nudb\npath=/var/db\nearliestseq=42\n");
        let db = cfg.node_db();
        assert!(db.backend_extras.contains_key("earliestseq"),
            "unknown key should land in backend_extras, not be silently lost");
        // The real "earliest_seq" is untouched by the typo key — it keeps its default.
        // (validate_lenient only enforces floor ≥ 1 when the field was explicitly set below 1.)
        let NodeDbConfig { earliest_seq, .. } = NodeDbConfig::default();
        assert_eq!(db.earliest_seq, earliest_seq,
            "typo key must not affect the real earliest_seq field");
    }

    // ---- F18: [transaction_queue] lenient clamping ----

    #[test]
    fn txq_normal_consensus_increase_above_1000_clamped() {
        // 1001 exceeds the C++ ceiling of 1000 — must be silently clamped to 1000 in INI mode.
        let cfg = parse("[transaction_queue]\nnormal_consensus_increase_percent=1001\n");
        assert_eq!(cfg.parsed.transaction_queue.normal_consensus_increase_percent, 1000);
    }

    #[test]
    fn txq_slow_consensus_decrease_above_100_clamped() {
        // 200 exceeds the C++ ceiling of 100 — must be silently clamped to 100 in INI mode.
        let cfg = parse("[transaction_queue]\nslow_consensus_decrease_percent=200\n");
        assert_eq!(cfg.parsed.transaction_queue.slow_consensus_decrease_percent, 100);
    }

    #[test]
    fn txq_percent_fields_in_range_unchanged() {
        // Values within valid range must pass through unmodified.
        let cfg = parse(
            "[transaction_queue]\nnormal_consensus_increase_percent=500\nslow_consensus_decrease_percent=75\n",
        );
        assert_eq!(cfg.parsed.transaction_queue.normal_consensus_increase_percent, 500);
        assert_eq!(cfg.parsed.transaction_queue.slow_consensus_decrease_percent, 75);
    }

    // ---- F38: [crawl] mixed kv + bare lines ----

    /// When `[crawl]` has both kv pairs and bare values, the kv path wins (Detailed)
    /// and the bare value is silently dropped — matching C++ behavior where
    /// `lookup_["overlay"]` takes precedence over `values_=["true"]`.
    #[test]
    fn crawl_mixed_kv_and_bare_kv_wins() {
        let cfg = parse("[crawl]\ntrue\noverlay=1\n");
        // has_kv = true → Detailed path; bare "true" is discarded.
        assert_eq!(
            cfg.crawl(),
            &CrawlConfig::Detailed { overlay: true, server: false, counts: false, unl: false },
        );
    }

    #[test]
    fn crawl_mixed_bare_then_kv() {
        // Order shouldn't matter — presence of any kv line forces Detailed.
        let cfg = parse("[crawl]\nserver=1\ntrue\n");
        assert_eq!(
            cfg.crawl(),
            &CrawlConfig::Detailed { overlay: false, server: true, counts: false, unl: false },
        );
    }
}

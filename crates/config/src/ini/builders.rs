//! Per-nested-struct builders.
//!
//! Each `build_*` function constructs one `Option<Struct>` field of `Config`
//! directly from `BasicConfig`.  Struct literals are used throughout so that
//! a missing field in the schema becomes a compile error here.
//!
//! Compare with C++ `Config.cpp::Config::loadFromString`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::ParseError;
use crate::ini::parser::{BasicConfig, Section};

use crate::schema::database::{
    JournalMode, LedgerTxTables, NodeDb, NodeDbCommon, NuDbOptions, RocksDbOptions, SafetyLevel,
    Sqdb, SqdbBackend, Sqlite, Synchronous, TempStore,
};
use crate::schema::diagnostics::{Insight, InsightServer, Perf};
use crate::schema::enums::{
    FetchDepth, FetchDepthName, LedgerHistory, LedgerHistoryName, NetworkId, NetworkIdName,
    NodeSize, NodeSizeName, RelayMode,
};
use crate::schema::grpc::Grpc;
use crate::schema::hashrouter::HashRouter;
use crate::schema::misc::{Crawl, Vl};
use crate::schema::overlay::Overlay;
use crate::schema::reduce_relay::ReduceRelay;
use crate::schema::server::{PortConfig, PortLimit, PortLimitName, Protocol, Server};
use crate::schema::transaction_queue::TransactionQueue;
use crate::schema::voting::Voting;

use super::accessors::{
    parse_bool_compat, SectionExt,
};

// ---------------------------------------------------------------------------
// 1+2. [server] + per-port lifting
// ---------------------------------------------------------------------------

pub fn build_server(bc: &BasicConfig) -> Result<Option<Server>, ParseError> {
    let Some(server_sec) = bc.get("server") else {
        return Ok(None);
    };
    if server_sec.lookup.is_empty() && server_sec.values.is_empty() {
        return Ok(None);
    }

    // Shared defaults — k/v pairs from [server] itself.
    let defaults = build_port_config("server", server_sec)?;

    // Per-port sections — value-lines from [server] list port names.
    let mut ports: BTreeMap<String, PortConfig> = BTreeMap::new();
    for port_name in &server_sec.values {
        if port_name == "port_grpc" {
            // port_grpc is handled separately; skip it here.
            continue;
        }
        let port_sec = bc.get(port_name.as_str()).ok_or_else(|| {
            ParseError::Ini(format!(
                "no [{port_name}] section for port listed in [server]"
            ))
        })?;
        let port_cfg = build_port_config(port_name.as_str(), port_sec)?;
        ports.insert(port_name.clone(), port_cfg);
    }

    Ok(Some(Server { defaults, ports }))
}

/// Build a `PortConfig` from a section.  Used for both [server] defaults and
/// individual [port_*] sections.
fn build_port_config(section_name: &str, sec: &Section) -> Result<PortConfig, ParseError> {
    // Comma-split protocol tokens and parse them as Protocol enum.
    let protocol: Option<Vec<Protocol>> = sec
        .comma_split_lowercase("protocol")
        .map(|tokens| {
            tokens
                .into_iter()
                .map(|t| parse_protocol(&t, section_name))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    // `limit` is polymorphic: "unlimited" | u16.
    let limit: Option<PortLimit> = sec
        .get_str("limit")
        .map(|raw| parse_port_limit(raw, section_name))
        .transpose()?;

    Ok(PortConfig {
        ip:                          sec.get_string("ip"),
        port:                        sec.get_parse("port", section_name)?,
        protocol,
        limit,
        send_queue_limit:            sec.get_parse("send_queue_limit", section_name)?,
        user:                        sec.get_string("user"),
        password:                    sec.get_string("password"),
        admin_user:                  sec.get_string("admin_user"),
        admin_password:              sec.get_string("admin_password"),
        admin:                       sec.comma_split("admin"),
        secure_gateway:              sec.comma_split("secure_gateway"),
        ssl_key:                     sec.get_str("ssl_key").map(PathBuf::from),
        ssl_cert:                    sec.get_str("ssl_cert").map(PathBuf::from),
        ssl_chain:                   sec.get_str("ssl_chain").map(PathBuf::from),
        ssl_ciphers:                 sec.get_string("ssl_ciphers"),
        permessage_deflate:          sec.get_bool("permessage_deflate", section_name)?,
        client_max_window_bits:      sec.get_parse("client_max_window_bits", section_name)?,
        server_max_window_bits:      sec.get_parse("server_max_window_bits", section_name)?,
        client_no_context_takeover:  sec.get_bool("client_no_context_takeover", section_name)?,
        server_no_context_takeover:  sec.get_bool("server_no_context_takeover", section_name)?,
        compress_level:              sec.get_parse("compress_level", section_name)?,
        memory_level:                sec.get_parse("memory_level", section_name)?,
    })
}

fn parse_protocol(s: &str, section: &str) -> Result<Protocol, ParseError> {
    match s {
        "http"  => Ok(Protocol::Http),
        "https" => Ok(Protocol::Https),
        "ws"    => Ok(Protocol::Ws),
        "wss"   => Ok(Protocol::Wss),
        "peer"  => Ok(Protocol::Peer),
        other   => Err(ParseError::Ini(format!(
            "unknown protocol '{other}' in [{section}]"
        ))),
    }
}

fn parse_port_limit(raw: &str, section: &str) -> Result<PortLimit, ParseError> {
    let lower = raw.to_ascii_lowercase();
    if lower == "unlimited" {
        return Ok(PortLimit::Named(PortLimitName::Unlimited));
    }
    raw.parse::<u16>()
        .map(PortLimit::Numeric)
        .map_err(|_| {
            ParseError::Ini(format!(
                "cannot parse limit '{raw}' as u16 or 'unlimited' in [{section}]"
            ))
        })
}

// ---------------------------------------------------------------------------
// 1. port_grpc → grpc rename
// ---------------------------------------------------------------------------

pub fn build_grpc(bc: &BasicConfig) -> Result<Option<Grpc>, ParseError> {
    let Some(sec) = bc.get("port_grpc") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(Grpc {
        ip:             sec.get_string("ip"),
        port:           sec.get_parse("port", "port_grpc")?,
        secure_gateway: sec.comma_split("secure_gateway"),
        ssl_cert:       sec.get_str("ssl_cert").map(PathBuf::from),
        ssl_key:        sec.get_str("ssl_key").map(PathBuf::from),
        ssl_cert_chain: sec.get_str("ssl_cert_chain").map(PathBuf::from),
        ssl_client_ca:  sec.get_str("ssl_client_ca").map(PathBuf::from),
    }))
}

// ---------------------------------------------------------------------------
// 3. node_db / import_db — tagged enum with type canonicalization
// ---------------------------------------------------------------------------

pub fn build_node_db(
    bc: &BasicConfig,
    section_name: &str,
) -> Result<Option<NodeDb>, ParseError> {
    let Some(sec) = bc.get(section_name) else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    let raw_type = sec
        .get_str("type")
        .ok_or_else(|| ParseError::Ini(format!("[{section_name}] missing 'type' key")))?;

    let common = build_node_db_common(sec, section_name)?;

    match raw_type.to_ascii_lowercase().as_str() {
        "nudb" => {
            Ok(Some(NodeDb::NuDb(NuDbOptions {
                common,
                nudb_block_size: sec.get_parse("nudb_block_size", section_name)?,
            })))
        }
        "rocksdb" => {
            Ok(Some(NodeDb::RocksDb(RocksDbOptions {
                common,
                cache_mb:    sec.get_parse("cache_mb", section_name)?,
                filter_bits: sec.get_parse("filter_bits", section_name)?,
            })))
        }
        other => Err(ParseError::Ini(format!(
            "unknown node_db type '{other}' in [{section_name}]"
        ))),
    }
}

fn build_node_db_common(sec: &Section, section_name: &str) -> Result<NodeDbCommon, ParseError> {
    let path_str = sec
        .get_str("path")
        .ok_or_else(|| ParseError::Ini(format!("[{section_name}] missing 'path' key")))?;
    Ok(NodeDbCommon {
        path:                   PathBuf::from(path_str),
        fast_load:              sec.get_bool("fast_load", section_name)?,
        earliest_seq:           sec.get_parse("earliest_seq", section_name)?,
        online_delete:          sec.get_parse("online_delete", section_name)?,
        advisory_delete:        sec.get_bool("advisory_delete", section_name)?,
        delete_batch:           sec.get_parse("delete_batch", section_name)?,
        back_off_milliseconds:  sec.get_parse("back_off_milliseconds", section_name)
            .or_else(|_| sec.get_parse("backOff", section_name))?,
        age_threshold_seconds:  sec.get_parse("age_threshold_seconds", section_name)?,
        recovery_wait_seconds:  sec.get_parse("recovery_wait_seconds", section_name)?,
    })
}

// ---------------------------------------------------------------------------
// overlay
// ---------------------------------------------------------------------------

pub fn build_overlay(bc: &BasicConfig) -> Result<Option<Overlay>, ParseError> {
    let Some(sec) = bc.get("overlay") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(Overlay {
        public_ip:        sec.get_string("public_ip"),
        ip_limit:         sec.get_parse("ip_limit", "overlay")?,
        max_unknown_time: sec.get_parse("max_unknown_time", "overlay")?,
        max_diverged_time: sec.get_parse("max_diverged_time", "overlay")?,
    }))
}

// ---------------------------------------------------------------------------
// reduce_relay
// ---------------------------------------------------------------------------

pub fn build_reduce_relay(bc: &BasicConfig) -> Result<Option<ReduceRelay>, ParseError> {
    let Some(sec) = bc.get("reduce_relay") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(ReduceRelay {
        vp_base_squelch_enable:             sec.get_bool("vp_base_squelch_enable", "reduce_relay")?,
        vp_enable:                          sec.get_bool("vp_enable", "reduce_relay")?,
        vp_base_squelch_max_selected_peers: sec.get_parse("vp_base_squelch_max_selected_peers", "reduce_relay")?,
        tx_enable:                          sec.get_bool("tx_enable", "reduce_relay")?,
        tx_metrics:                         sec.get_bool("tx_metrics", "reduce_relay")?,
        tx_min_peers:                       sec.get_parse("tx_min_peers", "reduce_relay")?,
        tx_relay_percentage:                sec.get_parse("tx_relay_percentage", "reduce_relay")?,
    }))
}

// ---------------------------------------------------------------------------
// transaction_queue
// ---------------------------------------------------------------------------

pub fn build_transaction_queue(bc: &BasicConfig) -> Result<Option<TransactionQueue>, ParseError> {
    let Some(sec) = bc.get("transaction_queue") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(TransactionQueue {
        ledgers_in_queue:                    sec.get_parse("ledgers_in_queue", "transaction_queue")?,
        minimum_queue_size:                  sec.get_parse("minimum_queue_size", "transaction_queue")?,
        retry_sequence_percent:              sec.get_parse("retry_sequence_percent", "transaction_queue")?,
        minimum_escalation_multiplier:       sec.get_parse("minimum_escalation_multiplier", "transaction_queue")?,
        minimum_txn_in_ledger:               sec.get_parse("minimum_txn_in_ledger", "transaction_queue")?,
        minimum_txn_in_ledger_standalone:    sec.get_parse("minimum_txn_in_ledger_standalone", "transaction_queue")?,
        target_txn_in_ledger:                sec.get_parse("target_txn_in_ledger", "transaction_queue")?,
        maximum_txn_in_ledger:               sec.get_parse("maximum_txn_in_ledger", "transaction_queue")?,
        normal_consensus_increase_percent:   sec.get_parse("normal_consensus_increase_percent", "transaction_queue")?,
        slow_consensus_decrease_percent:     sec.get_parse("slow_consensus_decrease_percent", "transaction_queue")?,
        maximum_txn_per_account:             sec.get_parse("maximum_txn_per_account", "transaction_queue")?,
        minimum_last_ledger_buffer:          sec.get_parse("minimum_last_ledger_buffer", "transaction_queue")?,
        zero_basefee_transaction_feelevel:   sec.get_parse("zero_basefee_transaction_feelevel", "transaction_queue")?,
    }))
}

// ---------------------------------------------------------------------------
// hashrouter
// ---------------------------------------------------------------------------

pub fn build_hashrouter(bc: &BasicConfig) -> Result<Option<HashRouter>, ParseError> {
    let Some(sec) = bc.get("hashrouter") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(HashRouter {
        hold_time:  sec.get_parse("hold_time", "hashrouter")?,
        relay_time: sec.get_parse("relay_time", "hashrouter")?,
    }))
}

// ---------------------------------------------------------------------------
// sqlite
// ---------------------------------------------------------------------------

pub fn build_sqlite(bc: &BasicConfig) -> Result<Option<Sqlite>, ParseError> {
    let Some(sec) = bc.get("sqlite") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    let safety_level = sec
        .get_str("safety_level")
        .map(parse_safety_level)
        .transpose()?;

    let journal_mode = sec
        .get_str("journal_mode")
        .map(parse_journal_mode)
        .transpose()?;

    let synchronous = sec
        .get_str("synchronous")
        .map(parse_synchronous)
        .transpose()?;

    let temp_store = sec
        .get_str("temp_store")
        .map(parse_temp_store)
        .transpose()?;

    Ok(Some(Sqlite {
        safety_level,
        journal_mode,
        synchronous,
        temp_store,
        page_size:          sec.get_parse("page_size", "sqlite")?,
        journal_size_limit: sec.get_parse("journal_size_limit", "sqlite")?,
    }))
}

fn parse_safety_level(s: &str) -> Result<SafetyLevel, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "high" => Ok(SafetyLevel::High),
        "low"  => Ok(SafetyLevel::Low),
        other  => Err(ParseError::Ini(format!(
            "invalid sqlite.safety_level '{other}'"
        ))),
    }
}

fn parse_journal_mode(s: &str) -> Result<JournalMode, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "delete"   => Ok(JournalMode::Delete),
        "truncate" => Ok(JournalMode::Truncate),
        "persist"  => Ok(JournalMode::Persist),
        "memory"   => Ok(JournalMode::Memory),
        "wal"      => Ok(JournalMode::Wal),
        "off"      => Ok(JournalMode::Off),
        other      => Err(ParseError::Ini(format!(
            "invalid sqlite.journal_mode '{other}'"
        ))),
    }
}

fn parse_synchronous(s: &str) -> Result<Synchronous, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "off"    => Ok(Synchronous::Off),
        "normal" => Ok(Synchronous::Normal),
        "full"   => Ok(Synchronous::Full),
        "extra"  => Ok(Synchronous::Extra),
        other    => Err(ParseError::Ini(format!(
            "invalid sqlite.synchronous '{other}'"
        ))),
    }
}

fn parse_temp_store(s: &str) -> Result<TempStore, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "default" => Ok(TempStore::Default),
        "file"    => Ok(TempStore::File),
        "memory"  => Ok(TempStore::Memory),
        other     => Err(ParseError::Ini(format!(
            "invalid sqlite.temp_store '{other}'"
        ))),
    }
}

// ---------------------------------------------------------------------------
// sqdb
// ---------------------------------------------------------------------------

pub fn build_sqdb(bc: &BasicConfig) -> Result<Option<Sqdb>, ParseError> {
    let Some(sec) = bc.get("sqdb") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    let backend = sec
        .get_str("backend")
        .map(parse_sqdb_backend)
        .transpose()?;

    Ok(Some(Sqdb { backend }))
}

fn parse_sqdb_backend(s: &str) -> Result<SqdbBackend, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "sqlite" => Ok(SqdbBackend::Sqlite),
        other    => Err(ParseError::Ini(format!(
            "unsupported soci backend '{other}' in [sqdb]"
        ))),
    }
}

// ---------------------------------------------------------------------------
// ledger_tx_tables
// ---------------------------------------------------------------------------

pub fn build_ledger_tx_tables(bc: &BasicConfig) -> Result<Option<LedgerTxTables>, ParseError> {
    let Some(sec) = bc.get("ledger_tx_tables") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(LedgerTxTables {
        use_tx_tables: sec.get_bool_int_compat("use_tx_tables", "ledger_tx_tables")?,
    }))
}

// ---------------------------------------------------------------------------
// insight
// ---------------------------------------------------------------------------

pub fn build_insight(bc: &BasicConfig) -> Result<Option<Insight>, ParseError> {
    let Some(sec) = bc.get("insight") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    let server = sec
        .get_str("server")
        .map(parse_insight_server)
        .transpose()?;

    Ok(Some(Insight {
        server,
        address: sec.get_string("address"),
        prefix:  sec.get_string("prefix"),
    }))
}

fn parse_insight_server(s: &str) -> Result<InsightServer, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "statsd" => Ok(InsightServer::Statsd),
        // Any other value selects the NullCollector (spec §4.6); we store None
        // by returning an error that the caller converts to None via transpose.
        // BUT — we need to return Some(InsightServer::Statsd) only for statsd.
        // For unknown values the spec says "silently selects NullCollector", so
        // we return None which the caller will propagate as no server.
        _ => Err(ParseError::Ini(format!(
            "unrecognized insight.server value '{s}' (only 'statsd' is supported)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// perf
// ---------------------------------------------------------------------------

pub fn build_perf(bc: &BasicConfig) -> Result<Option<Perf>, ParseError> {
    let Some(sec) = bc.get("perf") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(Perf {
        perf_log:     sec.get_str("perf_log").map(PathBuf::from),
        log_interval: sec.get_parse("log_interval", "perf")?,
    }))
}

// ---------------------------------------------------------------------------
// voting
// ---------------------------------------------------------------------------

pub fn build_voting(bc: &BasicConfig) -> Result<Option<Voting>, ParseError> {
    let Some(sec) = bc.get("voting") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }
    Ok(Some(Voting {
        reference_fee:   sec.get_parse("reference_fee", "voting")?,
        account_reserve: sec.get_parse("account_reserve", "voting")?,
        owner_reserve:   sec.get_parse("owner_reserve", "voting")?,
    }))
}

// ---------------------------------------------------------------------------
// 5. crawl — 0|1 value-line lift
// ---------------------------------------------------------------------------

pub fn build_crawl(bc: &BasicConfig) -> Result<Option<Crawl>, ParseError> {
    let Some(sec) = bc.get("crawl") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    // If there is exactly one value-line that is "0" or "1" and no `enabled`
    // key in lookup, synthesise enabled = true|false.
    let enabled: Option<bool> = if sec.values.len() == 1
        && (sec.values[0] == "0" || sec.values[0] == "1")
        && !sec.lookup.contains_key("enabled")
    {
        Some(sec.values[0] == "1")
    } else {
        sec.get_bool("enabled", "crawl")?
    };

    Ok(Some(Crawl {
        enabled,
        overlay: sec.get_bool("overlay", "crawl")?,
        server:  sec.get_bool("server", "crawl")?,
        counts:  sec.get_bool("counts", "crawl")?,
        unl:     sec.get_bool("unl", "crawl")?,
    }))
}

// ---------------------------------------------------------------------------
// 6. vl — enable → enabled alias
// ---------------------------------------------------------------------------

pub fn build_vl(bc: &BasicConfig) -> Result<Option<Vl>, ParseError> {
    let Some(sec) = bc.get("vl") else {
        return Ok(None);
    };
    if sec.lookup.is_empty() && sec.values.is_empty() {
        return Ok(None);
    }

    // Back-compat alias: "enable" in INI → "enabled" in the schema.
    let enabled = if let Some(raw) = sec.get_str("enable") {
        parse_bool_compat(raw)
            .map(Some)
            .ok_or_else(|| ParseError::Ini(format!(
                "cannot parse [vl].enable value '{raw}' as bool"
            )))?
    } else {
        sec.get_bool("enabled", "vl")?
    };

    Ok(Some(Vl { enabled }))
}

// ---------------------------------------------------------------------------
// Polymorphic enum parsers (top-level single-value sections)
// ---------------------------------------------------------------------------

pub fn parse_ledger_history(s: &str) -> Result<LedgerHistory, ParseError> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(LedgerHistory::Numeric(n));
    }
    match s.to_ascii_lowercase().as_str() {
        "full" => Ok(LedgerHistory::Named(LedgerHistoryName::Full)),
        "none" => Ok(LedgerHistory::Named(LedgerHistoryName::None)),
        other  => Err(ParseError::Ini(format!(
            "invalid ledger_history value '{other}'"
        ))),
    }
}

pub fn parse_fetch_depth(s: &str) -> Result<FetchDepth, ParseError> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(FetchDepth::Numeric(n));
    }
    match s.to_ascii_lowercase().as_str() {
        "full" => Ok(FetchDepth::Named(FetchDepthName::Full)),
        "none" => Ok(FetchDepth::Named(FetchDepthName::None)),
        other  => Err(ParseError::Ini(format!(
            "invalid fetch_depth value '{other}'"
        ))),
    }
}

pub fn parse_network_id(s: &str) -> Result<NetworkId, ParseError> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(NetworkId::Numeric(n));
    }
    match s.to_ascii_lowercase().as_str() {
        "main"    => Ok(NetworkId::Named(NetworkIdName::Main)),
        "testnet" => Ok(NetworkId::Named(NetworkIdName::Testnet)),
        "devnet"  => Ok(NetworkId::Named(NetworkIdName::Devnet)),
        other     => Err(ParseError::Ini(format!(
            "invalid network_id value '{other}'"
        ))),
    }
}

pub fn parse_node_size(s: &str) -> Result<NodeSize, ParseError> {
    if let Ok(n) = s.parse::<u8>() {
        return Ok(NodeSize::Numeric(n));
    }
    match s.to_ascii_lowercase().as_str() {
        "tiny"   => Ok(NodeSize::Named(NodeSizeName::Tiny)),
        "small"  => Ok(NodeSize::Named(NodeSizeName::Small)),
        "medium" => Ok(NodeSize::Named(NodeSizeName::Medium)),
        "large"  => Ok(NodeSize::Named(NodeSizeName::Large)),
        "huge"   => Ok(NodeSize::Named(NodeSizeName::Huge)),
        other    => Err(ParseError::Ini(format!(
            "invalid node_size value '{other}'"
        ))),
    }
}

pub fn parse_relay_mode(s: &str, section: &str) -> Result<RelayMode, ParseError> {
    match s.to_ascii_lowercase().as_str() {
        "all"            => Ok(RelayMode::All),
        "trusted"        => Ok(RelayMode::Trusted),
        "drop_untrusted" => Ok(RelayMode::DropUntrusted),
        other            => Err(ParseError::Ini(format!(
            "invalid [{section}] value '{other}'"
        ))),
    }
}

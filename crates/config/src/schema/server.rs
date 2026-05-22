//! `[server]` + per-port (`[server.ports.<name>]`) schema.
//!
//! `[server]` carries shared defaults that are inherited by each named port at
//! load time. Every per-port field is optional in the schema; the
//! required-ness of `ip`, `port`, and `protocol` is enforced post-deserialize
//! after defaults have been merged in.

use std::collections::BTreeMap;
use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Server {
    /// Shared defaults applied to every port unless overridden.
    #[serde(flatten)]
    pub defaults: PortConfig,

    /// Per-port sections, keyed by section name (e.g. `port_peer`).
    // FFI (phase 2): map flattening — planned shape is `Vec<NamedPort>` where
    // `NamedPort { name: String, config: Box<PortConfig> }`. Planned getters:
    // `Server::port_names()` returning `&[String]` and `Server::port(name)`
    // returning `Result<&PortConfig>`.
    #[serde(default)]
    #[config_entry(skip)]
    pub ports: BTreeMap<String, PortConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    pub ip: Option<String>,
    pub port: Option<u16>,
    // FFI (phase 2): `Vec<Protocol>` — needs a cxx-shared `Protocol` enum
    // (Http|Https|Ws|Wss|Peer). Planned: `PortConfig::protocols()` returning
    // `&[Protocol]` (empty when absent, like other `Option<Vec<…>>` fields).
    #[config_entry(skip)]
    pub protocol: Option<Vec<Protocol>>,
    // FFI (phase 2): polymorphic — `PortLimit` is `"unlimited" | u16`. Planned:
    // `PortConfig::limit_kind()` + `limit_value()` (kind = Unlimited|Numeric).
    #[config_entry(skip)]
    pub limit: Option<PortLimit>,
    pub send_queue_limit: Option<u16>,

    pub user: Option<String>,
    pub password: Option<String>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,

    /// IPv4/IPv6 addresses or CIDR networks granted admin access.
    pub admin: Option<Vec<String>>,
    /// IPv4/IPv6 addresses or CIDR networks treated as a secure gateway.
    pub secure_gateway: Option<Vec<String>>,

    pub ssl_key: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
    pub ssl_chain: Option<PathBuf>,
    pub ssl_ciphers: Option<String>,

    pub permessage_deflate: Option<bool>,
    pub client_max_window_bits: Option<u8>,
    pub server_max_window_bits: Option<u8>,
    pub client_no_context_takeover: Option<bool>,
    pub server_no_context_takeover: Option<bool>,
    pub compress_level: Option<u8>,
    pub memory_level: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
    Ws,
    Wss,
    Peer,
}

/// Per-port connection limit: either an explicit cap or `"unlimited"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PortLimit {
    Named(PortLimitName),
    Numeric(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PortLimitName {
    Unlimited,
}

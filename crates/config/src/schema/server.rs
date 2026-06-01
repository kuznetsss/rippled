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

use crate::ffi;

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Server {
    /// Shared defaults applied to every port unless overridden.
    #[serde(flatten)]
    pub defaults: PortConfig,

    /// Per-port sections, keyed by section name (e.g. `port_peer`).
    // FFI: `Server::port_names()` / `has_port()` / `port()` below.
    #[serde(default)]
    #[config_entry(skip)]
    pub ports: BTreeMap<String, PortConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    pub ip: Option<String>,
    pub port: Option<u16>,
    // FFI: `PortConfig::protocols()` below (returns empty `Vec` when absent).
    #[config_entry(skip)]
    pub protocol: Option<Vec<Protocol>>,
    // FFI: `PortConfig::limit()` below; returns `OptionalPortLimit`.
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

impl From<PortLimitName> for ffi::PortLimitKind {
    fn from(value: PortLimitName) -> Self {
        match value {
            PortLimitName::Unlimited => ffi::PortLimitKind::Unlimited,
        }
    }
}

// ---- FFI projection types ----
//
// These live next to the schema types they wrap, imported into `ffi.rs`'s
// scope so cxx-bridge can resolve `super::OptionalT`.

impl From<Protocol> for ffi::Protocol {
    fn from(v: Protocol) -> ffi::Protocol {
        match v {
            Protocol::Http => ffi::Protocol::Http,
            Protocol::Https => ffi::Protocol::Https,
            Protocol::Ws => ffi::Protocol::Ws,
            Protocol::Wss => ffi::Protocol::Wss,
            Protocol::Peer => ffi::Protocol::Peer,
        }
    }
}

pub struct OptionalPortLimit(Option<PortLimit>);

impl From<Option<PortLimit>> for OptionalPortLimit {
    fn from(v: Option<PortLimit>) -> Self {
        Self(v)
    }
}

impl OptionalPortLimit {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn kind(&self) -> Result<ffi::PortLimitKind, String> {
        match self.0 {
            Some(PortLimit::Named(name)) => Ok(name.into()),
            Some(PortLimit::Numeric(_)) => Ok(ffi::PortLimitKind::Numeric),
            None => Err("OptionalPortLimit has no value".into()),
        }
    }

    pub fn numeric_value(&self) -> Result<u16, String> {
        match self.0 {
            Some(PortLimit::Numeric(n)) => Ok(n),
            Some(_) => Err("OptionalPortLimit is not Numeric".into()),
            None => Err("OptionalPortLimit has no value".into()),
        }
    }
}

// ---- Inherent getters on schema types ----

impl Server {
    /// Names of every per-port section in sorted order (the underlying
    /// container is a `BTreeMap`, so iteration is already sorted).
    pub fn port_names(&self) -> Vec<String> {
        self.ports.keys().cloned().collect()
    }

    pub fn has_port(&self, name: &str) -> bool {
        self.ports.contains_key(name)
    }

    /// Lookup a named port. Throws across FFI when no port with that name
    /// exists — programmer error, same semantics as the rest of the surface.
    pub fn port(&self, name: &str) -> Result<&PortConfig, String> {
        self.ports
            .get(name)
            .ok_or_else(|| format!("config: no server.ports.{name}"))
    }
}

impl PortConfig {
    /// Empty `Vec` when the field is absent — matches the
    /// `Option<Vec<String>>` convention used elsewhere in the bridge for
    /// list-shaped optional fields.
    pub fn protocols(&self) -> Vec<ffi::Protocol> {
        match &self.protocol {
            Some(v) => v.iter().copied().map(Into::into).collect(),
            None => Vec::new(),
        }
    }

    pub fn limit(&self) -> Box<OptionalPortLimit> {
        Box::new(self.limit.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ffi::{PortLimitKind, Protocol};

    fn ok_outcome(s: &str) -> Box<crate::schema::Config> {
        let (cfg, _) = crate::parse_from_str(s, crate::ConfigFormat::Toml)
            .expect("parse succeeded")
            .finalize()
            .expect("finalize succeeded");
        Box::new(cfg)
    }

    // ----- Server::ports map -----

    #[test]
    fn server_port_names_sorted_and_lookup_round_trip() {
        let cfg = ok_outcome(
            r#"
                [server]
                send_queue_limit = 500

                [server.ports.port_rpc]
                ip       = "127.0.0.1"
                port     = 5005
                protocol = ["http", "https"]
                limit    = 200

                [server.ports.port_peer]
                ip       = "0.0.0.0"
                port     = 51235
                protocol = ["peer"]
                limit    = "unlimited"
            "#,
        );
        let srv = cfg.server().unwrap();
        let names = srv.port_names();
        // BTreeMap iteration order is lexicographic.
        assert_eq!(names, vec!["port_peer".to_string(), "port_rpc".to_string()]);
        assert!(srv.has_port("port_peer"));
        assert!(srv.has_port("port_rpc"));
        assert!(!srv.has_port("nope"));

        let peer = srv.port("port_peer").unwrap();
        assert_eq!(peer.port().value().unwrap(), 51235);
        let rpc = srv.port("port_rpc").unwrap();
        assert_eq!(rpc.port().value().unwrap(), 5005);
    }

    #[test]
    fn server_port_missing_throws() {
        let cfg = ok_outcome(
            r#"
                [server]
                send_queue_limit = 500
            "#,
        );
        let srv = cfg.server().unwrap();
        assert!(srv.port_names().is_empty());
        let err = srv.port("missing").unwrap_err();
        assert!(err.contains("missing"), "{err}");
    }

    // ----- PortConfig protocols + limit -----

    #[test]
    fn port_protocols_returned_in_order() {
        let cfg = ok_outcome(
            r#"
                [server.ports.port_rpc]
                ip       = "127.0.0.1"
                port     = 5005
                protocol = ["http", "https", "ws"]
            "#,
        );
        let rpc = cfg.server().unwrap().port("port_rpc").unwrap();
        let ps = rpc.protocols();
        assert_eq!(ps.len(), 3);
        assert!(matches!(ps[0], Protocol::Http));
        assert!(matches!(ps[1], Protocol::Https));
        assert!(matches!(ps[2], Protocol::Ws));
    }

    #[test]
    fn port_protocols_empty_when_absent() {
        let cfg = ok_outcome(
            r#"
                [server.ports.port_rpc]
                ip   = "127.0.0.1"
                port = 5005
            "#,
        );
        let rpc = cfg.server().unwrap().port("port_rpc").unwrap();
        assert!(rpc.protocols().is_empty());
    }

    #[test]
    fn port_limit_unlimited_kind() {
        let cfg = ok_outcome(
            r#"
                [server.ports.port_peer]
                ip    = "0.0.0.0"
                port  = 51235
                limit = "unlimited"
            "#,
        );
        let peer = cfg.server().unwrap().port("port_peer").unwrap();
        let lim = peer.limit();
        assert!(lim.has_value());
        assert!(matches!(lim.kind().unwrap(), PortLimitKind::Unlimited));
        // Numeric accessor throws when the kind isn't Numeric.
        assert!(lim.numeric_value().is_err());
    }

    #[test]
    fn port_limit_numeric_kind() {
        let cfg = ok_outcome(
            r#"
                [server.ports.port_rpc]
                ip    = "127.0.0.1"
                port  = 5005
                limit = 200
            "#,
        );
        let rpc = cfg.server().unwrap().port("port_rpc").unwrap();
        let lim = rpc.limit();
        assert!(matches!(lim.kind().unwrap(), PortLimitKind::Numeric));
        assert_eq!(lim.numeric_value().unwrap(), 200u16);
    }

    #[test]
    fn port_limit_absent() {
        let cfg = ok_outcome(
            r#"
                [server.ports.port_rpc]
                ip   = "127.0.0.1"
                port = 5005
            "#,
        );
        let rpc = cfg.server().unwrap().port("port_rpc").unwrap();
        let lim = rpc.limit();
        assert!(!lim.has_value());
        assert!(lim.kind().is_err());
        assert!(lim.numeric_value().is_err());
    }
}

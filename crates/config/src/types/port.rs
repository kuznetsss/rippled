use std::net::IpAddr;
use std::path::PathBuf;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

/// Protocols that a port can serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortProtocol {
    Http,
    Https,
    Ws,
    Wss,
    Peer,
    Grpc,
}

/// A connection limit — either unlimited or a specific count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortLimit {
    Unlimited,
    Count(u64),
}

impl Default for PortLimit {
    fn default() -> Self {
        PortLimit::Unlimited
    }
}

/// Default values that apply to every port unless overridden per-port.
/// Defined in `[server]` kv pairs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortDefaults {
    pub ip: Option<IpAddr>,
    pub protocol: Vec<PortProtocol>,
    pub admin: Vec<IpNet>,
    pub secure_gateway: Vec<IpNet>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub limit: PortLimit,
    /// Must be > 0. Default 100.
    pub send_queue_limit: u16,
    pub ssl_key: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
    pub ssl_chain: Option<PathBuf>,
    pub ssl_ciphers: Option<String>,
    pub ssl_cert_chain: Option<PathBuf>,
    pub ssl_client_ca: Option<PathBuf>,
    /// Default true.
    pub permessage_deflate: bool,
    /// Range 9..=15. Default 15.
    pub client_max_window_bits: u8,
    /// Range 9..=15. Default 15.
    pub server_max_window_bits: u8,
    pub client_no_context_takeover: bool,
    pub server_no_context_takeover: bool,
    /// Range 0..=9. Default 8.
    pub compress_level: u8,
    /// Range 1..=9. Default 4.
    pub memory_level: u8,
}

impl Default for PortDefaults {
    fn default() -> Self {
        PortDefaults {
            ip: None,
            protocol: Vec::new(),
            admin: Vec::new(),
            secure_gateway: Vec::new(),
            user: None,
            password: None,
            admin_user: None,
            admin_password: None,
            limit: PortLimit::Unlimited,
            send_queue_limit: 100,
            ssl_key: None,
            ssl_cert: None,
            ssl_chain: None,
            ssl_ciphers: None,
            ssl_cert_chain: None,
            ssl_client_ca: None,
            permessage_deflate: true,
            client_max_window_bits: 15,
            server_max_window_bits: 15,
            client_no_context_takeover: false,
            server_no_context_takeover: false,
            compress_level: 8,
            memory_level: 4,
        }
    }
}

/// Configuration for a single named port (from `[port_<name>]` in INI or
/// `[port.<name>]` in TOML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortConfig {
    /// The user-chosen port name (matches the entry in `ServerConfig::port_names`).
    pub name: String,
    /// The TCP port number. Must be > 0 (enforced by `checkZeroPorts`).
    pub port: u16,
    /// The effective settings for this port after merging server-level defaults.
    pub effective: PortDefaults,
}

impl Default for PortConfig {
    fn default() -> Self {
        PortConfig {
            name: String::new(),
            port: 0,
            effective: PortDefaults::default(),
        }
    }
}

/// The `[server]` section: a list of port names (bare lines) plus shared
/// defaults (kv pairs) applied to every port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Names of the port subsections, in source order.
    pub port_names: Vec<String>,
    /// Server-level defaults applied to each port before per-port overrides.
    pub defaults: PortDefaults,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port_names: Vec::new(),
            defaults: PortDefaults::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_defaults_default_values() {
        let d = PortDefaults::default();
        assert_eq!(d.send_queue_limit, 100);
        assert_eq!(d.client_max_window_bits, 15);
        assert_eq!(d.server_max_window_bits, 15);
        assert!(d.permessage_deflate);
        assert_eq!(d.compress_level, 8);
        assert_eq!(d.memory_level, 4);
        assert!(!d.client_no_context_takeover);
        assert!(!d.server_no_context_takeover);
        assert!(d.ip.is_none());
        assert!(d.protocol.is_empty());
        assert!(d.admin.is_empty());
        assert!(d.ssl_key.is_none());
    }

    #[test]
    fn port_defaults_default_passes_strict_validation() {
        PortDefaults::default()
            .validate_strict("test")
            .expect("default should be valid");
    }

    #[test]
    fn port_limit_default_is_unlimited() {
        assert_eq!(PortLimit::default(), PortLimit::Unlimited);
    }

    #[test]
    fn port_config_default_port_is_zero() {
        let c = PortConfig::default();
        assert_eq!(c.port, 0);
        assert_eq!(c.name, "");
    }

    #[test]
    fn port_defaults_send_queue_limit_zero_fails() {
        let mut d = PortDefaults::default();
        d.send_queue_limit = 0;
        let err = d.validate_strict("port.rpc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("send_queue_limit"), "got: {msg}");
    }

    #[test]
    fn port_defaults_send_queue_limit_one_ok() {
        let mut d = PortDefaults::default();
        d.send_queue_limit = 1;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_client_max_window_bits_boundary_min() {
        let mut d = PortDefaults::default();
        d.client_max_window_bits = 9;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_client_max_window_bits_boundary_max() {
        let mut d = PortDefaults::default();
        d.client_max_window_bits = 15;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_client_max_window_bits_too_low() {
        let mut d = PortDefaults::default();
        d.client_max_window_bits = 8;
        let err = d.validate_strict("port.rpc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("client_max_window_bits"), "got: {msg}");
    }

    #[test]
    fn port_defaults_client_max_window_bits_too_high() {
        let mut d = PortDefaults::default();
        d.client_max_window_bits = 16;
        assert!(d.validate_strict("port.rpc").is_err());
    }

    #[test]
    fn port_defaults_server_max_window_bits_too_low() {
        let mut d = PortDefaults::default();
        d.server_max_window_bits = 8;
        assert!(d.validate_strict("port.rpc").is_err());
    }

    #[test]
    fn port_defaults_compress_level_boundary_zero() {
        let mut d = PortDefaults::default();
        d.compress_level = 0;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_compress_level_boundary_nine() {
        let mut d = PortDefaults::default();
        d.compress_level = 9;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_compress_level_too_high() {
        let mut d = PortDefaults::default();
        d.compress_level = 10;
        let err = d.validate_strict("port.rpc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("compress_level"), "got: {msg}");
    }

    #[test]
    fn port_defaults_memory_level_boundary_one() {
        let mut d = PortDefaults::default();
        d.memory_level = 1;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_memory_level_boundary_nine() {
        let mut d = PortDefaults::default();
        d.memory_level = 9;
        assert!(d.validate_strict("port.rpc").is_ok());
    }

    #[test]
    fn port_defaults_memory_level_too_low() {
        let mut d = PortDefaults::default();
        d.memory_level = 0;
        let err = d.validate_strict("port.rpc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("memory_level"), "got: {msg}");
    }

    #[test]
    fn port_defaults_memory_level_too_high() {
        let mut d = PortDefaults::default();
        d.memory_level = 10;
        assert!(d.validate_strict("port.rpc").is_err());
    }

    #[test]
    fn port_config_validate_strict_port_zero_fails() {
        let c = PortConfig {
            name: "rpc".to_owned(),
            port: 0,
            effective: PortDefaults::default(),
        };
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rpc"), "got: {msg}");
    }

    #[test]
    fn port_config_validate_strict_valid() {
        let c = PortConfig {
            name: "rpc".to_owned(),
            port: 6006,
            effective: PortDefaults::default(),
        };
        assert!(c.validate_strict().is_ok());
    }
}

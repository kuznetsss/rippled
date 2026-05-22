//! `[grpc]` (hoisted from the legacy `[port_grpc]` section).

use std::path::PathBuf;

use config_derive::ConfigEntries;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, ConfigEntries)]
#[serde(deny_unknown_fields)]
pub struct Grpc {
    pub ip: Option<String>,
    pub port: Option<u32>,

    /// CSV of IP addresses recognized as a secure gateway. Unspecified
    /// addresses (`0.0.0.0`, `::`) are rejected during validation.
    pub secure_gateway: Option<Vec<String>>,

    pub ssl_cert: Option<PathBuf>,
    pub ssl_key: Option<PathBuf>,
    pub ssl_cert_chain: Option<PathBuf>,
    pub ssl_client_ca: Option<PathBuf>,
}

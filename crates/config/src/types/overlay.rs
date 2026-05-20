use std::net::IpAddr;
use serde::{Deserialize, Serialize};

/// Overlay (peer-to-peer) network configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Advertised public IP address. `None` = auto-detect.
    pub public_ip: Option<IpAddr>,
    /// Maximum number of inbound connections from the same IP. `None` = auto.
    pub ip_limit: Option<u32>,
    /// Seconds before an unknown peer is dropped. Range 300..=1800. Default 600.
    pub max_unknown_time: u32,
    /// Seconds before a diverged peer is dropped. Range 60..=900. Default 300.
    pub max_diverged_time: u32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        OverlayConfig {
            public_ip: None,
            ip_limit: None,
            max_unknown_time: 600,
            max_diverged_time: 300,
        }
    }
}

/// Reduce-relay configuration from `[reduce_relay]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReduceRelayConfig {
    pub vp_base_squelch_enable: bool,
    /// Must be >= 3.
    pub vp_base_squelch_max_selected_peers: u32,
    pub tx_enable: bool,
    pub tx_metrics: bool,
    /// Must be >= 10.
    pub tx_min_peers: u32,
    /// Range 10..=100.
    pub tx_relay_percentage: u32,
}

impl Default for ReduceRelayConfig {
    fn default() -> Self {
        ReduceRelayConfig {
            vp_base_squelch_enable: false,
            vp_base_squelch_max_selected_peers: 5,
            tx_enable: false,
            tx_metrics: false,
            tx_min_peers: 20,
            tx_relay_percentage: 25,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- OverlayConfig defaults and validation ----

    #[test]
    fn overlay_default_values() {
        let c = OverlayConfig::default();
        assert_eq!(c.max_unknown_time, 600);
        assert_eq!(c.max_diverged_time, 300);
        assert_eq!(c.public_ip, None);
        assert_eq!(c.ip_limit, None);
    }

    #[test]
    fn overlay_default_passes_strict_validation() {
        OverlayConfig::default().validate_strict().expect("default should be valid");
    }

    #[test]
    fn overlay_max_unknown_time_boundary_min() {
        let mut c = OverlayConfig::default();
        c.max_unknown_time = 300;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn overlay_max_unknown_time_boundary_max() {
        let mut c = OverlayConfig::default();
        c.max_unknown_time = 1800;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn overlay_max_unknown_time_too_low() {
        let mut c = OverlayConfig::default();
        c.max_unknown_time = 299;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_unknown_time"), "got: {msg}");
    }

    #[test]
    fn overlay_max_unknown_time_too_high() {
        let mut c = OverlayConfig::default();
        c.max_unknown_time = 1801;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_unknown_time"), "got: {msg}");
    }

    #[test]
    fn overlay_max_diverged_time_boundary_min() {
        let mut c = OverlayConfig::default();
        c.max_diverged_time = 60;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn overlay_max_diverged_time_boundary_max() {
        let mut c = OverlayConfig::default();
        c.max_diverged_time = 900;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn overlay_max_diverged_time_too_low() {
        let mut c = OverlayConfig::default();
        c.max_diverged_time = 59;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_diverged_time"), "got: {msg}");
    }

    #[test]
    fn overlay_max_diverged_time_too_high() {
        let mut c = OverlayConfig::default();
        c.max_diverged_time = 901;
        assert!(c.validate_strict().is_err());
    }

    // ---- ReduceRelayConfig defaults and validation ----

    #[test]
    fn reduce_relay_default_values() {
        let c = ReduceRelayConfig::default();
        assert!(!c.vp_base_squelch_enable);
        assert_eq!(c.vp_base_squelch_max_selected_peers, 5);
        assert!(!c.tx_enable);
        assert!(!c.tx_metrics);
        assert_eq!(c.tx_min_peers, 20);
        assert_eq!(c.tx_relay_percentage, 25);
    }

    #[test]
    fn reduce_relay_default_passes_strict_validation() {
        ReduceRelayConfig::default().validate_strict().expect("default should be valid");
    }

    #[test]
    fn reduce_relay_squelch_max_peers_boundary_min() {
        let mut c = ReduceRelayConfig::default();
        c.vp_base_squelch_max_selected_peers = 3;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn reduce_relay_squelch_max_peers_too_low() {
        let mut c = ReduceRelayConfig::default();
        c.vp_base_squelch_max_selected_peers = 2;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("vp_base_squelch_max_selected_peers"), "got: {msg}");
    }

    #[test]
    fn reduce_relay_tx_min_peers_boundary_min() {
        let mut c = ReduceRelayConfig::default();
        c.tx_min_peers = 10;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn reduce_relay_tx_min_peers_too_low() {
        let mut c = ReduceRelayConfig::default();
        c.tx_min_peers = 9;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("tx_min_peers"), "got: {msg}");
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_boundary_min() {
        let mut c = ReduceRelayConfig::default();
        c.tx_relay_percentage = 10;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_boundary_max() {
        let mut c = ReduceRelayConfig::default();
        c.tx_relay_percentage = 100;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_too_low() {
        let mut c = ReduceRelayConfig::default();
        c.tx_relay_percentage = 9;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("tx_relay_percentage"), "got: {msg}");
    }

    #[test]
    fn reduce_relay_tx_relay_percentage_too_high() {
        let mut c = ReduceRelayConfig::default();
        c.tx_relay_percentage = 101;
        assert!(c.validate_strict().is_err());
    }
}

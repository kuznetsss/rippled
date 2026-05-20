use serde::{Deserialize, Serialize};

/// Transaction queue configuration from `[transaction_queue]`.
/// All numeric clamps from analysis §5 are applied during INI adapt (lenient)
/// or strict TOML validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TxQConfig {
    /// Number of ledgers to keep transactions queued. Default 20.
    pub ledgers_in_queue: u32,
    /// Minimum number of transactions in the queue before escalation. Default 2000.
    pub minimum_queue_size: u32,
    /// Percentage of the base fee to use as a retry fee. Default 25.
    pub retry_sequence_percent: u32,
    /// Minimum escalation multiplier (× kBASE_LEVEL). Default 500.
    pub minimum_escalation_multiplier: u32,
    /// Minimum transactions per ledger in normal mode. Default 32.
    pub minimum_txn_in_ledger: u32,
    /// Minimum transactions per ledger in standalone mode. Default 1000.
    pub minimum_txn_in_ledger_standalone: u32,
    /// Target transactions per ledger. Default 256.
    pub target_txn_in_ledger: u32,
    /// Maximum transactions per ledger. `None` = unlimited; must be >= min when set.
    pub maximum_txn_in_ledger: Option<u32>,
    /// Percent increase per ledger during normal consensus. Clamped 0..=1000. Default 20.
    pub normal_consensus_increase_percent: u32,
    /// Percent decrease per ledger during slow consensus. Clamped 0..=100. Default 50.
    pub slow_consensus_decrease_percent: u32,
    /// Maximum queued transactions per account. Default 10.
    pub maximum_txn_per_account: u32,
    /// Minimum buffer between last ledger sequence and queue expiry. Default 2.
    pub minimum_last_ledger_buffer: u32,
    /// Fee level for zero-base-fee transactions. Default 256000.
    pub zero_basefee_transaction_feelevel: u32,
}

impl TxQConfig {
    /// Apply silent INI clamps per analysis §5:
    /// - `normal_consensus_increase_percent` clamped to 0..=1000
    /// - `slow_consensus_decrease_percent` clamped to 0..=100
    pub(crate) fn validate_lenient(&mut self) {
        self.normal_consensus_increase_percent =
            self.normal_consensus_increase_percent.clamp(0, 1000);
        self.slow_consensus_decrease_percent =
            self.slow_consensus_decrease_percent.clamp(0, 100);
    }
}

impl Default for TxQConfig {
    fn default() -> Self {
        TxQConfig {
            ledgers_in_queue: 20,
            minimum_queue_size: 2000,
            retry_sequence_percent: 25,
            minimum_escalation_multiplier: 500,
            minimum_txn_in_ledger: 32,
            minimum_txn_in_ledger_standalone: 1000,
            target_txn_in_ledger: 256,
            maximum_txn_in_ledger: None,
            normal_consensus_increase_percent: 20,
            slow_consensus_decrease_percent: 50,
            maximum_txn_per_account: 10,
            minimum_last_ledger_buffer: 2,
            zero_basefee_transaction_feelevel: 256000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txq_default_values() {
        let c = TxQConfig::default();
        assert_eq!(c.ledgers_in_queue, 20);
        assert_eq!(c.minimum_queue_size, 2000);
        assert_eq!(c.retry_sequence_percent, 25);
        assert_eq!(c.minimum_escalation_multiplier, 500);
        assert_eq!(c.minimum_txn_in_ledger, 32);
        assert_eq!(c.minimum_txn_in_ledger_standalone, 1000);
        assert_eq!(c.target_txn_in_ledger, 256);
        assert_eq!(c.maximum_txn_in_ledger, None);
        assert_eq!(c.normal_consensus_increase_percent, 20);
        assert_eq!(c.slow_consensus_decrease_percent, 50);
        assert_eq!(c.maximum_txn_per_account, 10);
        assert_eq!(c.minimum_last_ledger_buffer, 2);
        assert_eq!(c.zero_basefee_transaction_feelevel, 256000);
    }

    #[test]
    fn txq_default_passes_strict_validation() {
        TxQConfig::default().validate_strict().expect("default should be valid");
    }

    #[test]
    fn txq_normal_consensus_increase_boundary_zero() {
        let mut c = TxQConfig::default();
        c.normal_consensus_increase_percent = 0;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_normal_consensus_increase_boundary_1000() {
        let mut c = TxQConfig::default();
        c.normal_consensus_increase_percent = 1000;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_normal_consensus_increase_too_high() {
        let mut c = TxQConfig::default();
        c.normal_consensus_increase_percent = 1001;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("normal_consensus_increase_percent"), "got: {msg}");
    }

    #[test]
    fn txq_slow_consensus_decrease_boundary_zero() {
        let mut c = TxQConfig::default();
        c.slow_consensus_decrease_percent = 0;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_slow_consensus_decrease_boundary_100() {
        let mut c = TxQConfig::default();
        c.slow_consensus_decrease_percent = 100;
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_slow_consensus_decrease_too_high() {
        let mut c = TxQConfig::default();
        c.slow_consensus_decrease_percent = 101;
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("slow_consensus_decrease_percent"), "got: {msg}");
    }

    #[test]
    fn txq_maximum_txn_in_ledger_valid() {
        let mut c = TxQConfig::default();
        c.minimum_txn_in_ledger = 32;
        c.maximum_txn_in_ledger = Some(64);
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_maximum_txn_in_ledger_equal_to_min_ok() {
        let mut c = TxQConfig::default();
        c.minimum_txn_in_ledger = 32;
        c.maximum_txn_in_ledger = Some(32);
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn txq_maximum_txn_in_ledger_less_than_min_fails() {
        let mut c = TxQConfig::default();
        c.minimum_txn_in_ledger = 32;
        c.maximum_txn_in_ledger = Some(31);
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("maximum_txn_in_ledger"), "got: {msg}");
    }

    #[test]
    fn txq_maximum_txn_in_ledger_none_ok() {
        let mut c = TxQConfig::default();
        c.maximum_txn_in_ledger = None;
        assert!(c.validate_strict().is_ok());
    }
}

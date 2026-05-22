//! `[transaction_queue]` table. EXPERIMENTAL upstream.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionQueue {
    pub ledgers_in_queue: Option<u32>,
    pub minimum_queue_size: Option<u32>,
    pub retry_sequence_percent: Option<u32>,
    pub minimum_escalation_multiplier: Option<u32>,
    pub minimum_txn_in_ledger: Option<u32>,
    pub minimum_txn_in_ledger_standalone: Option<u32>,
    pub target_txn_in_ledger: Option<u32>,
    /// Must be `>=` both `minimum_txn_in_ledger` and
    /// `minimum_txn_in_ledger_standalone` when set.
    pub maximum_txn_in_ledger: Option<u32>,
    /// Clamped to `[0, 1000]`. Default `20`.
    pub normal_consensus_increase_percent: Option<u32>,
    /// Clamped to `[0, 100]`. Default `50`.
    pub slow_consensus_decrease_percent: Option<u32>,
    pub maximum_txn_per_account: Option<u32>,
    pub minimum_last_ledger_buffer: Option<u32>,
    pub zero_basefee_transaction_feelevel: Option<u64>,
}

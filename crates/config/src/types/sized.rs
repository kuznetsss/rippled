/// The five node-size buckets, used to index into `SIZED_TABLE`.
/// `#[repr(u8)]` so it can be cast to an index directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum NodeSize {
    Tiny = 0,
    Small,
    Medium,
    Large,
    Huge,
}

impl Default for NodeSize {
    fn default() -> Self {
        NodeSize::Tiny
    }
}

/// Identifiers for the 13 runtime-tunable items in `SIZED_TABLE`.
/// The order **must** match the C++ `kSIZED_ITEMS` array (enforced by the
/// static assertion in `Config.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SizedItem {
    SweepInterval = 0,
    TreeCacheSize,
    TreeCacheAge,
    LedgerSize,
    LedgerAge,
    LedgerFetch,
    HashNodeDbCache,
    TxnDbCache,
    LgrDbCache,
    OpenFinalLimit,
    BurstSize,
    RamSizeGb,
    AccountIdCacheSize,
}

/// Values copied verbatim from `src/xrpld/core/detail/Config.cpp:114-137`.
/// Layout: `SIZED_TABLE[SizedItem as usize][NodeSize as usize]`.
///
/// Column order: `tiny`, `small`, `medium`, `large`, `huge`.
pub const SIZED_TABLE: [[i32; 5]; 13] = [
    /* SweepInterval      */ [10, 30, 60, 90, 120],
    /* TreeCacheSize      */ [262144, 524288, 2097152, 4194304, 8388608],
    /* TreeCacheAge       */ [30, 60, 90, 120, 900],
    /* LedgerSize         */ [32, 32, 64, 256, 384],
    /* LedgerAge          */ [30, 60, 180, 300, 600],
    /* LedgerFetch        */ [2, 3, 4, 5, 8],
    /* HashNodeDbCache    */ [4, 12, 24, 64, 128],
    /* TxnDbCache         */ [4, 12, 24, 64, 128],
    /* LgrDbCache         */ [4, 8, 16, 32, 128],
    /* OpenFinalLimit     */ [8, 16, 32, 64, 128],
    /* BurstSize          */ [4, 8, 16, 32, 48],
    /* RamSizeGb          */ [6, 8, 12, 24, 0],
    /* AccountIdCacheSize */ [20047, 50053, 77081, 150061, 300007],
];

/// Look up the value for `item` at `size`.
pub const fn sized_value(item: SizedItem, size: NodeSize) -> i32 {
    SIZED_TABLE[item as usize][size as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_interval_values() {
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Tiny), 10);
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Huge), 120);
    }

    #[test]
    fn account_id_cache_values() {
        assert_eq!(
            sized_value(SizedItem::AccountIdCacheSize, NodeSize::Tiny),
            20047
        );
        assert_eq!(
            sized_value(SizedItem::AccountIdCacheSize, NodeSize::Huge),
            300007
        );
    }

    #[test]
    fn table_dimensions() {
        // 13 items × 5 sizes
        assert_eq!(SIZED_TABLE.len(), 13);
        for row in &SIZED_TABLE {
            assert_eq!(row.len(), 5);
        }
    }

    // ---- additional coverage ----

    // Cross-check every row against the C++ kSIZED_ITEMS values from Config.cpp:114-137.
    #[test]
    fn sweep_interval_all_sizes() {
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Tiny),   10);
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Small),  30);
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Medium), 60);
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Large),  90);
        assert_eq!(sized_value(SizedItem::SweepInterval, NodeSize::Huge),  120);
    }

    #[test]
    fn tree_cache_size_all_sizes() {
        assert_eq!(sized_value(SizedItem::TreeCacheSize, NodeSize::Tiny),     262144);
        assert_eq!(sized_value(SizedItem::TreeCacheSize, NodeSize::Small),    524288);
        assert_eq!(sized_value(SizedItem::TreeCacheSize, NodeSize::Medium), 2097152);
        assert_eq!(sized_value(SizedItem::TreeCacheSize, NodeSize::Large),  4194304);
        assert_eq!(sized_value(SizedItem::TreeCacheSize, NodeSize::Huge),   8388608);
    }

    #[test]
    fn tree_cache_age_all_sizes() {
        assert_eq!(sized_value(SizedItem::TreeCacheAge, NodeSize::Tiny),    30);
        assert_eq!(sized_value(SizedItem::TreeCacheAge, NodeSize::Small),   60);
        assert_eq!(sized_value(SizedItem::TreeCacheAge, NodeSize::Medium),  90);
        assert_eq!(sized_value(SizedItem::TreeCacheAge, NodeSize::Large),  120);
        assert_eq!(sized_value(SizedItem::TreeCacheAge, NodeSize::Huge),   900);
    }

    #[test]
    fn ledger_size_all_sizes() {
        assert_eq!(sized_value(SizedItem::LedgerSize, NodeSize::Tiny),    32);
        assert_eq!(sized_value(SizedItem::LedgerSize, NodeSize::Small),   32);
        assert_eq!(sized_value(SizedItem::LedgerSize, NodeSize::Medium),  64);
        assert_eq!(sized_value(SizedItem::LedgerSize, NodeSize::Large),  256);
        assert_eq!(sized_value(SizedItem::LedgerSize, NodeSize::Huge),   384);
    }

    #[test]
    fn ledger_age_all_sizes() {
        assert_eq!(sized_value(SizedItem::LedgerAge, NodeSize::Tiny),    30);
        assert_eq!(sized_value(SizedItem::LedgerAge, NodeSize::Small),   60);
        assert_eq!(sized_value(SizedItem::LedgerAge, NodeSize::Medium), 180);
        assert_eq!(sized_value(SizedItem::LedgerAge, NodeSize::Large),  300);
        assert_eq!(sized_value(SizedItem::LedgerAge, NodeSize::Huge),   600);
    }

    #[test]
    fn ledger_fetch_all_sizes() {
        assert_eq!(sized_value(SizedItem::LedgerFetch, NodeSize::Tiny),   2);
        assert_eq!(sized_value(SizedItem::LedgerFetch, NodeSize::Small),  3);
        assert_eq!(sized_value(SizedItem::LedgerFetch, NodeSize::Medium), 4);
        assert_eq!(sized_value(SizedItem::LedgerFetch, NodeSize::Large),  5);
        assert_eq!(sized_value(SizedItem::LedgerFetch, NodeSize::Huge),   8);
    }

    #[test]
    fn hash_node_db_cache_all_sizes() {
        assert_eq!(sized_value(SizedItem::HashNodeDbCache, NodeSize::Tiny),    4);
        assert_eq!(sized_value(SizedItem::HashNodeDbCache, NodeSize::Small),  12);
        assert_eq!(sized_value(SizedItem::HashNodeDbCache, NodeSize::Medium), 24);
        assert_eq!(sized_value(SizedItem::HashNodeDbCache, NodeSize::Large),  64);
        assert_eq!(sized_value(SizedItem::HashNodeDbCache, NodeSize::Huge),  128);
    }

    #[test]
    fn txn_db_cache_all_sizes() {
        assert_eq!(sized_value(SizedItem::TxnDbCache, NodeSize::Tiny),    4);
        assert_eq!(sized_value(SizedItem::TxnDbCache, NodeSize::Small),  12);
        assert_eq!(sized_value(SizedItem::TxnDbCache, NodeSize::Medium), 24);
        assert_eq!(sized_value(SizedItem::TxnDbCache, NodeSize::Large),  64);
        assert_eq!(sized_value(SizedItem::TxnDbCache, NodeSize::Huge),  128);
    }

    #[test]
    fn lgr_db_cache_all_sizes() {
        assert_eq!(sized_value(SizedItem::LgrDbCache, NodeSize::Tiny),    4);
        assert_eq!(sized_value(SizedItem::LgrDbCache, NodeSize::Small),   8);
        assert_eq!(sized_value(SizedItem::LgrDbCache, NodeSize::Medium), 16);
        assert_eq!(sized_value(SizedItem::LgrDbCache, NodeSize::Large),  32);
        assert_eq!(sized_value(SizedItem::LgrDbCache, NodeSize::Huge),  128);
    }

    #[test]
    fn open_final_limit_all_sizes() {
        assert_eq!(sized_value(SizedItem::OpenFinalLimit, NodeSize::Tiny),    8);
        assert_eq!(sized_value(SizedItem::OpenFinalLimit, NodeSize::Small),  16);
        assert_eq!(sized_value(SizedItem::OpenFinalLimit, NodeSize::Medium), 32);
        assert_eq!(sized_value(SizedItem::OpenFinalLimit, NodeSize::Large),  64);
        assert_eq!(sized_value(SizedItem::OpenFinalLimit, NodeSize::Huge),  128);
    }

    #[test]
    fn burst_size_all_sizes() {
        assert_eq!(sized_value(SizedItem::BurstSize, NodeSize::Tiny),    4);
        assert_eq!(sized_value(SizedItem::BurstSize, NodeSize::Small),   8);
        assert_eq!(sized_value(SizedItem::BurstSize, NodeSize::Medium), 16);
        assert_eq!(sized_value(SizedItem::BurstSize, NodeSize::Large),  32);
        assert_eq!(sized_value(SizedItem::BurstSize, NodeSize::Huge),   48);
    }

    #[test]
    fn ram_size_gb_all_sizes() {
        assert_eq!(sized_value(SizedItem::RamSizeGb, NodeSize::Tiny),    6);
        assert_eq!(sized_value(SizedItem::RamSizeGb, NodeSize::Small),   8);
        assert_eq!(sized_value(SizedItem::RamSizeGb, NodeSize::Medium), 12);
        assert_eq!(sized_value(SizedItem::RamSizeGb, NodeSize::Large),  24);
        assert_eq!(sized_value(SizedItem::RamSizeGb, NodeSize::Huge),    0);  // huge = "auto"
    }

    #[test]
    fn account_id_cache_all_sizes() {
        assert_eq!(sized_value(SizedItem::AccountIdCacheSize, NodeSize::Tiny),    20047);
        assert_eq!(sized_value(SizedItem::AccountIdCacheSize, NodeSize::Small),   50053);
        assert_eq!(sized_value(SizedItem::AccountIdCacheSize, NodeSize::Medium),  77081);
        assert_eq!(sized_value(SizedItem::AccountIdCacheSize, NodeSize::Large),  150061);
        assert_eq!(sized_value(SizedItem::AccountIdCacheSize, NodeSize::Huge),   300007);
    }

    #[test]
    fn node_size_repr_u8_roundtrip() {
        assert_eq!(NodeSize::Tiny   as u8, 0);
        assert_eq!(NodeSize::Small  as u8, 1);
        assert_eq!(NodeSize::Medium as u8, 2);
        assert_eq!(NodeSize::Large  as u8, 3);
        assert_eq!(NodeSize::Huge   as u8, 4);
    }

    #[test]
    fn sized_item_repr_u8_roundtrip() {
        assert_eq!(SizedItem::SweepInterval      as u8,  0);
        assert_eq!(SizedItem::TreeCacheSize      as u8,  1);
        assert_eq!(SizedItem::TreeCacheAge       as u8,  2);
        assert_eq!(SizedItem::LedgerSize         as u8,  3);
        assert_eq!(SizedItem::LedgerAge          as u8,  4);
        assert_eq!(SizedItem::LedgerFetch        as u8,  5);
        assert_eq!(SizedItem::HashNodeDbCache    as u8,  6);
        assert_eq!(SizedItem::TxnDbCache         as u8,  7);
        assert_eq!(SizedItem::LgrDbCache         as u8,  8);
        assert_eq!(SizedItem::OpenFinalLimit     as u8,  9);
        assert_eq!(SizedItem::BurstSize          as u8, 10);
        assert_eq!(SizedItem::RamSizeGb          as u8, 11);
        assert_eq!(SizedItem::AccountIdCacheSize as u8, 12);
    }

    #[test]
    fn node_size_default_is_tiny() {
        assert_eq!(NodeSize::default(), NodeSize::Tiny);
    }
}

pub mod crawl;
pub mod duration;
pub mod fees;
pub mod hostport;
pub mod insight;
pub mod ledger;
pub mod node_db;
pub mod overlay;
pub mod path;
pub mod port;
pub mod relay;
pub mod sized;
pub mod sqlite;
pub mod startup;
pub mod txq;
pub mod validators;

// Flat re-exports so callers can write `use config::types::HostPort` etc.
pub use crawl::{CrawlConfig, VlConfig};
pub use duration::parse_amendment_majority_time;
pub use fees::VotingConfig;
pub use hostport::{HostKind, HostPort};
pub use insight::{InsightConfig, InsightServer, PerfConfig};
pub use ledger::{FetchDepth, LedgerHistory, LedgerTxTablesConfig};
pub use node_db::{NodeDbConfig, NodeDbKind};
pub use overlay::{OverlayConfig, ReduceRelayConfig};
pub use path::{resolve_against, RelPath};
pub use port::{PortConfig, PortDefaults, PortLimit, PortProtocol, ServerConfig};
pub use relay::RelayPolicy;
pub use sized::{sized_value, NodeSize, SizedItem, SIZED_TABLE};
pub use sqlite::{
    SqliteConfig, SqliteJournalMode, SqliteMode, SqliteSafety, SqliteSynchronous, SqliteTempStore,
};
pub use startup::StartUpType;
pub use txq::TxQConfig;
pub use validators::{ClusterNode, KnownAmendment, TrustedValidator};

/// A feature name from `[features]`. Stored as a bare string; validation
/// against registered feature names is a Phase 3 concern.
pub type FeatureName = String;

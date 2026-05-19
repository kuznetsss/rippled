# Config Rewrite — Step 2 Design

Companion to [`config_rewrite.md`](./config_rewrite.md) and [`config_rewrite_analysis.md`](./config_rewrite_analysis.md). This document is the contract that step 3 (Rust implementation) will build against.

The analysis doc made the **policy** decisions (lenient INI / strict TOML, two-stage INI parse, parsed-values vs. CLI-overrides split, `kSIZED_ITEMS` moves to Rust, etc.). This doc makes the **structural** decisions: crate layout, the exact types in the schema, parsing pipelines, FFI surface, and how C++ consumes it.

Decisions reached during design review are recorded in §15 as a log, so step 3 implementers can see which alternatives were considered.

Convention: file paths are repo-relative; line numbers are at the time of writing and may drift. Rust paths are written `crates/config/src/…`.

## 0. What's already settled

From the plan doc and analysis, the following are inputs to this design and not reopened here:

- **Full replacement** — Rust owns parsing and validation; C++ consumes a typed result. No long-lived parsing shim.
- **Asymmetric strictness** — INI compat-first / lenient; TOML strict. Same typed `Config` output.
- **INI strategy** — two-stage parse (raw section bag → typed sub-structs via per-section adapters).
- **TOML strategy** — `serde` directly, with a canonical schema.
- **Grammar policy** — case-sensitive identifiers; INI booleans `0|1|true|false`; INI numbers decimal-only; per-field duration grammars; piecemeal path resolution in INI; uniform path resolution in TOML.
- **One public `Config` type** — internally it carries three buckets (parsed file values, overrides, finalized/derived values from `bootstrap()`) but they're private. The user-facing surface is constructors, override setters, an explicit `bootstrap()`, and getters. The internal `overrides.X.unwrap_or(parsed.X)` resolution lives inside the getter implementations.
- **`validators.txt` splice** — merge into the main `Config`; overlap is an error in TOML, silent append in INI.
- **`kSIZED_ITEMS` + `NodeSize`** — table and auto-detection move to Rust.
- **Build integration** — already done. `crates/` is a Cargo workspace, integrated via Corrosion + cxx-rs; `add_xrpl_crate(rs_config CRATE config FILES lib.rs)` is wired in [crates/CMakeLists.txt:38](crates/CMakeLists.txt). Not revisited here.

What this design doc actually decides: the crate's module layout, every type in the schema, the exact INI parse pipeline, the FFI surface, the C++ consumption pattern, the error model, and the test strategy.

## 1. Crate layout

Single crate `config` (already declared in [crates/Cargo.toml](crates/Cargo.toml)). The principle: one public `Config` type; INI carries its parsing rules inside `ini/`; TOML is a thin serde layer; shared schema types live in `types/`.

```
crates/config/
├── Cargo.toml
└── src/
    ├── lib.rs              # re-exports, cxx::bridge mount
    ├── config.rs           # Config: constructors, override setters,
    │                       # bootstrap(), getters
    ├── error.rs            # ConfigError, spans, did-you-mean
    ├── types/              # everything returned by getters
    │   ├── mod.rs
    │   ├── port.rs         # PortConfig, PortDefaults, PortProtocol, PortLimit
    │   ├── node_db.rs      # NodeDbConfig, NodeDbKind
    │   ├── sqlite.rs       # SqliteConfig, SqliteMode and friends
    │   ├── overlay.rs      # OverlayConfig, ReduceRelayConfig
    │   ├── crawl.rs        # CrawlConfig (dual-shape)
    │   ├── txq.rs          # TxQConfig
    │   ├── fees.rs         # VotingConfig
    │   ├── validators.rs   # TrustedValidator, ClusterNode, KnownAmendment
    │   ├── insight.rs      # InsightConfig, PerfConfig
    │   ├── ledger.rs       # LedgerHistory, FetchDepth, LedgerTxTablesConfig
    │   ├── relay.rs        # RelayPolicy
    │   ├── startup.rs      # StartUpType
    │   ├── hostport.rs     # HostPort + its shared string parser
    │   ├── duration.rs     # amendment_majority_time grammar (shared by both formats)
    │   ├── path.rs         # RelPath wrapper, resolve-against-config-dir helper
    │   └── sized.rs        # NodeSize, SizedItem, kSIZED_ITEMS table
    ├── ini/
    │   ├── mod.rs          # parse_ini(&str) -> Result<Config, ConfigError>
    │   ├── lexer.rs        # line normalisation, comments, \# escape, section split
    │   ├── raw.rs          # RawSections / RawLine intermediate
    │   ├── grammar.rs      # INI-only: bool parser, decimal-int parser
    │   ├── serde.rs        # custom Deserializer over RawSection (kv map / bare seq)
    │   └── adapt.rs        # dispatch table + handwritten special-shape adapters
    ├── toml/
    │   ├── mod.rs          # parse_toml(&str) -> Result<Config, ConfigError>
    │   └── schema.rs       # serde structs (deny_unknown_fields) + From into Config
    ├── bootstrap.rs        # discover_config_file, validators.txt splice,
    │                       # mkdir -p, stderr echo, NodeSize auto-detection
    ├── ffi.rs              # cxx::bridge wrappers around Config
    └── tests/              # in-tree unit tests
```

Fixture files for integration tests live under `crates/config/tests/fixtures/`.

Public API surface (re-exported from `lib.rs`):

- `Config` — the single user-facing type. Constructors, override setters, `bootstrap()`, getters.
- All `types/*` sub-structs returned by getters.
- `ConfigError`, `ErrorSpan`, error kinds.
- `SizedItem`, `NodeSize`, `sized_value(item, node_size) -> i32`.

What's **not** public: `ini::raw`, `toml::schema`, the bucket structs inside `Config`, the format-specific parsing entrypoints. The user calls `Config::from_file`, not `parse_ini`.

The `cxx::bridge` module in `ffi.rs` is **not** the public Rust API — it's a thin wrapper that exposes the `Config` methods to C++ in cxx-compatible shapes. See §10.

### 1.1 Where the work happens

- `ini/` owns everything specific to INI: tokenisation, the bool/int grammar, the `\#` escape, dual-shape adapters for `[server]` and `[crawl]`, the two-stage section bag → typed field conversion.
- `toml/` is small. Serde structs mirror the canonical TOML schema with `deny_unknown_fields`; `From` impls walk them into `Config`.
- Both `ini::parse_ini` and `toml::parse_toml` return a fully populated `Config` — not a partial structure that needs further assembly. Validation that's identical between formats (range checks, mutual exclusion) lives on the `types/` structs as `try_from`/constructor logic so both formats share it. Validation that *differs* between formats (silent clamp vs. error for `max_transactions` etc.) lives in the format module.
- `bootstrap.rs` is the only place with filesystem side effects (env-var reads, `mkdir -p`, NodeSize RAM probe, `validators.txt` ingestion, `stderr` echo). Pure parsing in `ini/` and `toml/` has none of these.

## 2. The `Config` type

One public type. Three private buckets inside it. Lifecycle is `from_*` → optional `set_*` → `bootstrap()` → getters.

### 2.1 Public surface

```rust
pub struct Config { /* private */ }

impl Config {
    // -------- constructors --------

    /// Load + parse a file. Chooses format by extension (`.toml` → TOML,
    /// anything else → INI). Splices `validators.txt` if reachable.
    /// Does not run side-effect bootstrap; see `bootstrap()`.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError>;

    /// Parse an INI blob. No file discovery, no validators.txt splice
    /// (use `bootstrap()` to splice once a config dir is known).
    pub fn from_ini_str(text: &str) -> Result<Self, ConfigError>;

    /// Parse a TOML blob. Same caveats as `from_ini_str`.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError>;

    // -------- override setters (CLI + setup-control + test hooks) --------

    pub fn set_quiet(&mut self, v: bool);
    pub fn set_silent(&mut self, v: bool);
    pub fn set_standalone(&mut self, v: bool);
    pub fn set_start_up(&mut self, v: StartUpType);
    pub fn set_start_ledger(&mut self, v: String);
    pub fn set_start_valid(&mut self, v: bool);
    pub fn set_trap_tx_hash(&mut self, v: [u8; 32]);
    pub fn set_do_import(&mut self, v: bool);
    pub fn set_forced_ledger_range(&mut self, lo: u32, hi: u32);
    pub fn set_validation_quorum(&mut self, v: u64);
    pub fn set_rpc_ip(&mut self, v: SocketAddr);
    pub fn set_force_multi_thread(&mut self, v: bool);  // test hook

    // -------- finalize --------

    /// Run side effects: resolve paths against config dir, detect NodeSize,
    /// mkdir -p data_dir (unless standalone), echo path to stderr (unless
    /// quiet). Idempotent. Must run before any getter that touches
    /// resolved-path / NodeSize-derived values.
    pub fn bootstrap(&mut self) -> Result<(), ConfigError>;

    // -------- getters --------

    // top-level scalars (subset shown; one accessor per field)
    pub fn network_id(&self) -> u32;
    pub fn network_quorum(&self) -> u64;
    pub fn peer_private(&self) -> bool;
    pub fn ledger_history(&self) -> LedgerHistory;       // returns the effective
                                                          // value (standalone → None)
    pub fn fetch_depth(&self) -> FetchDepth;
    pub fn max_transactions(&self) -> i32;
    pub fn amendment_majority_time(&self) -> Duration;
    pub fn workers(&self) -> u32;
    pub fn io_workers(&self) -> u32;
    pub fn prefetch_workers(&self) -> u32;
    pub fn sweep_interval(&self) -> Option<u32>;
    pub fn server_domain(&self) -> Option<&str>;
    pub fn features(&self) -> &HashSet<FeatureName>;

    // post-bootstrap path getters
    pub fn config_dir(&self) -> &Path;
    pub fn data_dir(&self) -> &Path;
    pub fn debug_logfile(&self) -> Option<&Path>;
    pub fn validators_file(&self) -> Option<&Path>;
    pub fn node_size(&self) -> NodeSize;                 // effective

    // CLI-overridable accessors return the merged value
    pub fn start_up(&self) -> StartUpType;
    pub fn start_ledger(&self) -> Option<&str>;
    pub fn force_multi_thread(&self) -> bool;
    pub fn quiet(&self) -> bool;
    pub fn standalone(&self) -> bool;

    // sub-structs returned by-reference
    pub fn server(&self) -> &ServerConfig;
    pub fn port(&self, name: &str) -> Option<&PortConfig>;
    pub fn ports(&self) -> impl Iterator<Item = &PortConfig>;
    pub fn node_db(&self) -> &NodeDbConfig;
    pub fn import_db(&self) -> Option<&NodeDbConfig>;
    pub fn sqlite(&self) -> &SqliteConfig;
    pub fn overlay(&self) -> &OverlayConfig;
    pub fn reduce_relay(&self) -> &ReduceRelayConfig;
    pub fn crawl(&self) -> &CrawlConfig;
    pub fn voting(&self) -> &VotingConfig;
    pub fn transaction_queue(&self) -> &TxQConfig;
    pub fn insight(&self) -> &InsightConfig;
    pub fn perf(&self) -> &PerfConfig;
    pub fn ledger_tx_tables(&self) -> &LedgerTxTablesConfig;

    // bare-line lists
    pub fn ips(&self) -> &[HostPort];
    pub fn ips_fixed(&self) -> &[HostPort];
    pub fn sntp_servers(&self) -> &[String];
    pub fn cluster_nodes(&self) -> &[ClusterNode];
    pub fn amendments(&self) -> &[KnownAmendment];
    pub fn veto_amendments(&self) -> &[KnownAmendment];
    pub fn trusted_validators(&self) -> &[TrustedValidator];

    // sized-item table lookup
    pub fn sized_value(&self, item: SizedItem) -> i32;
    pub fn sized_value_for(&self, item: SizedItem, node: NodeSize) -> i32;
}
```

Typical use:

```rust
let mut cfg = Config::from_file(path)?;
if cli.quiet { cfg.set_quiet(true); }
if cli.standalone { cfg.set_standalone(true); }
if let Some(l) = cli.start_ledger { cfg.set_start_ledger(l); }
cfg.bootstrap()?;
// from here, cfg is fully baked.
let dir = cfg.data_dir();
let port = cfg.port("rpc_admin").ok_or(...)?;
```

### 2.2 What's inside (private)

```rust
pub struct Config {
    parsed: Parsed,            // file values, post-validators-splice
    overrides: Overrides,      // setter-driven; every field Option<T>
    finalized: Option<Finalized>, // None until bootstrap() runs
}

struct Parsed { /* all the file-derived fields — flat top-level list as in
                   §3, plus the typed sub-structs */ }

struct Overrides {
    quiet: Option<bool>,
    silent: Option<bool>,
    standalone: Option<bool>,
    start_up: Option<StartUpType>,
    start_ledger: Option<String>,
    start_valid: Option<bool>,
    trap_tx_hash: Option<[u8; 32]>,
    do_import: Option<bool>,
    forced_ledger_range: Option<(u32, u32)>,
    validation_quorum: Option<u64>,
    rpc_ip: Option<SocketAddr>,
    force_multi_thread: Option<bool>,    // analysis §2.5 / §7 #15
}

struct Finalized {
    config_dir: PathBuf,
    data_dir: PathBuf,
    debug_logfile_resolved: Option<PathBuf>,
    validators_file_resolved: Option<PathBuf>,
    node_size_effective: NodeSize,
    // forced-state derivations (LEDGER_HISTORY=0 if standalone,
    // path_search_max=0 if validator identity set, etc.) are *not* stored
    // here — getters compute them from parsed + overrides on each call.
}
```

Getter implementation pattern, for the three categories:

- **File-only field** (e.g. `peer_private`): `self.parsed.peer_private`.
- **CLI-overridable** (e.g. `start_up`): `self.overrides.start_up.unwrap_or(self.parsed.start_up)`.
- **Bootstrap-required** (e.g. `data_dir`, `node_size`): debug-asserts `finalized.is_some()`. In release, returns the value if finalized, panics with a clear message otherwise. (Production code always bootstraps; the panic is for catching test-code mistakes, not user-facing.)
- **Effective with forced override** (e.g. `ledger_history`): returns `LedgerHistory::None_` if `self.standalone()`, else `self.parsed.ledger_history`.

### 2.3 Field list

The full list of file-derived fields on `parsed: Parsed` is enumerated once in §3 (per section group) so it doesn't have to live in two places. The override-able fields are the ones with `set_*` methods above.

### 2.4 Naming note (analysis §7 #15)

The analysis left `CliOverrides` vs `ConfigOverrides` open. Since the umbrella now lives entirely inside `Config` as a private struct, the name is purely internal — using the short **`Overrides`**. The user-facing surface is `set_*` methods on `Config`; the struct name only matters to crate-internal code.

## 3. Schema details

Per-section sub-structs. Field-level details that already appear in the analysis are not repeated; this section captures the Rust type signatures and any decisions the analysis deferred.

### 3.1 `ServerConfig` and `PortConfig`

```rust
pub struct ServerConfig {
    pub port_names: Vec<String>,           // [server].values_ (INI) or server.ports (TOML)
    pub defaults: PortDefaults,            // [server] kv pairs, applied to each port
}

pub struct PortDefaults {
    pub ip: Option<IpAddr>,
    pub protocol: Vec<PortProtocol>,
    pub admin: Vec<IpNet>,                 // CIDR list
    pub secure_gateway: Vec<IpNet>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub limit: PortLimit,                  // Unlimited | Count(u64)
    pub send_queue_limit: u16,             // > 0
    pub ssl_key: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
    pub ssl_chain: Option<PathBuf>,
    pub ssl_ciphers: Option<String>,
    pub ssl_cert_chain: Option<PathBuf>,
    pub ssl_client_ca: Option<PathBuf>,
    pub permessage_deflate: bool,
    pub client_max_window_bits: u8,        // 9..=15
    pub server_max_window_bits: u8,        // 9..=15
    pub client_no_context_takeover: bool,
    pub server_no_context_takeover: bool,
    pub compress_level: u8,                // 0..=9
    pub memory_level: u8,                  // 1..=9
}

pub struct PortConfig {
    pub name: String,
    pub port: u16,                         // > 0 (analysis §5)
    // every PortDefaults field, overrideable per-port
    pub effective: PortDefaults,
}

#[derive(Clone, Copy)]
pub enum PortProtocol { Http, Https, Ws, Wss, Peer, Grpc }
```

Construction rule (applied during INI adapt + TOML convert):
1. Parse `[server]` into `(port_names, raw_defaults)`.
2. For each name in `port_names`, look up `[port_<name>]` (INI) or `port.<name>` (TOML).
3. Merge defaults under the per-port table, store the result as `PortConfig.effective`.
4. `checkZeroPorts` runs as a cross-section validator on the assembled config (no `port` field may be 0).
5. Stray ports (table with no matching name in `port_names`):
   - **INI lenient:** silently dropped (existing behavior).
   - **TOML strict:** error `OrphanPortTable { name }`.

Order: `BTreeMap` preserves no insertion order; `port_names: Vec<String>` is the canonical source-order listing. Callers that need source order iterate `server.port_names`; callers that need lookup use `ports.get(name)`.

### 3.2 `NodeDbConfig` and `import_db`

`[node_db]` and `[import_db]` share the exact schema. `import_db` only matters with `--import`, so it's stored as `Option<NodeDbConfig>` on `Parsed`.

```rust
pub struct NodeDbConfig {
    pub kind: NodeDbKind,                  // NuDb | RocksDb
    pub path: PathBuf,                     // not auto-resolved (analysis §6.6)
    pub fast_load: bool,                   // canonical home of FAST_LOAD
    pub earliest_seq: u32,                 // >= 1
    pub online_delete: Option<u32>,        // >= 256 when set, >= ledger_history (cross-section)
    pub advisory_delete: bool,
    pub delete_batch: u32,
    pub back_off_milliseconds: u32,
    pub age_threshold_seconds: u32,
    pub recovery_wait_seconds: u32,
    pub nudb_block_size: u32,              // power-of-2 in 4096..=32768
    // RocksDB-specific tunables: passed through verbatim as a key→string map,
    // since several are computed from NodeSize when unset and are consumed by
    // NodeStore::Manager rather than Config itself.
    pub backend_extras: BTreeMap<String, String>,
}
```

`backend_extras` is the one concession to "raw values": several RocksDB knobs (`cache_mb`, `filter_bits`, …) are read by `NodeStore` and never validated by Config today. They go through unchanged. INI lenient mode: any key not in the explicit list goes into `backend_extras`. TOML strict mode: only the known explicit list is accepted; `backend_extras` is populated only via a single `[node_db.extras]` sub-table.

### 3.3 `SqliteConfig`

```rust
pub struct SqliteConfig {
    pub mode: SqliteMode,
    pub journal_size_limit: i64,
}

pub enum SqliteMode {
    Safety { level: SqliteSafety },                  // safety_level
    Tuning {                                         // explicit triple
        journal_mode: Option<SqliteJournalMode>,
        synchronous: Option<SqliteSynchronous>,
        temp_store: Option<SqliteTempStore>,
        page_size: u32,                              // power-of-2 in 512..=65536
    },
    Default,                                         // neither block set
}
```

`SqliteMode` enforces the mutual exclusion from analysis §5: `safety_level` cannot coexist with the journal/synchronous/temp_store triple at the type level.

Enum values (`high`, `low`, `delete`, `wal`, …) are matched case-insensitively (per the §7 #4 carve-out for per-section enum values that today use `boost::iequals`).

### 3.4 `OverlayConfig`, `ReduceRelayConfig`, `CrawlConfig`, `VlConfig`

```rust
pub struct OverlayConfig {
    pub public_ip: Option<IpAddr>,
    pub ip_limit: Option<u32>,             // None = auto
    pub max_unknown_time: u32,             // 300..=1800
    pub max_diverged_time: u32,            // 60..=900
}

pub struct ReduceRelayConfig {
    pub vp_base_squelch_enable: bool,
    pub vp_base_squelch_max_selected_peers: u32,    // >= 3
    pub tx_enable: bool,
    pub tx_metrics: bool,
    pub tx_min_peers: u32,                          // >= 10
    pub tx_relay_percentage: u32,                   // 10..=100
}

pub enum CrawlConfig {
    LegacyBool(bool),                       // INI-only single-bool form
    Detailed {
        overlay: bool,
        server: bool,
        counts: bool,
        unl: bool,
    },
}

pub struct VlConfig { pub enabled: bool }
```

INI: `[crawl] true` (a bare-value line) deserialises to `LegacyBool(true)`; `[crawl]` with kv pairs deserialises to `Detailed`. Both are valid in INI per analysis §6.1. TOML: only `Detailed` is valid; the schema is `[crawl] overlay = … server = … counts = … unl = …`. `LegacyBool` is unrepresentable in TOML by construction.

### 3.5 `TxQConfig`

A flat struct mirroring the keys from analysis §3.7. No special shape. All numeric clamps from §5 become explicit validation steps (INI silently coerces, TOML errors).

### 3.6 `VotingConfig` and fees

```rust
pub struct VotingConfig {
    pub reference_fee: u64,
    pub account_reserve: u64,
    pub owner_reserve: u64,
}
```

The top-level `[fee_default]` single-line override of `reference_fee` is captured as a separate `fee_default: Option<u64>` field on `Parsed`, *not* applied to `voting.reference_fee` during parse — `bootstrap()` applies the override during finalization so the source of each value remains visible for diagnostics. The `Config::voting()` getter returns the already-resolved view.

### 3.7 Validators

```rust
pub struct TrustedValidator {
    pub key: String,            // base58 public key
    pub label: Option<String>,  // optional human-readable label
}

pub struct ClusterNode {
    pub key: String,
    pub label: Option<String>,
}

pub struct KnownAmendment {
    pub id: [u8; 32],           // 64-hex amendment ID
    pub name: String,
}
```

`[validators]` and `[validator_keys]` were consolidated in C++ (the latter's lines appended into the former). In Rust, **both feed the same `trusted_validators: Vec<TrustedValidator>`** during INI adapt / TOML convert. Consumers see one list. The bare-line grammar `<base58_key> [label]` is shared between `[validators]`, `[validator_keys]`, and `[cluster_nodes]`.

### 3.8 `InsightConfig`, `PerfConfig`, `LedgerTxTablesConfig`

```rust
pub struct InsightConfig {
    pub server: InsightServer,             // currently only StatsD
    pub address: Option<SocketAddr>,
    pub prefix: Option<String>,
}

pub struct PerfConfig {
    pub perf_log: Option<RelPath>,         // relative to config dir per existing behavior
    pub log_interval: u32,                 // seconds, default 1
}

pub struct LedgerTxTablesConfig {
    pub use_tx_tables: bool,               // canonical home of USE_TX_TABLES_
}
```

### 3.9 Top-level helper enums

```rust
pub enum RelayPolicy { All, Trusted, DropUntrusted }
pub enum LedgerHistory { Full, None_, Count(u32) }
pub enum FetchDepth { Full, None_, Count(u32) }
pub enum StartUpType { Normal, Load, Replay, NewChain, FromLedger, /* … */ }
pub enum NodeSize { Tiny, Small, Medium, Large, Huge }
pub enum NodeDbKind { NuDb, RocksDb }
pub enum InsightServer { StatsD }
```

`NodeSize` reuses the variants in `Config::SizedItem` numbering (`Tiny=0..Huge=4`) so the table lookup in §8 is index-by-cast. The exact numeric mapping must round-trip through cxx; see §10.

## 4. Grammar primitives

Shared types and parsers live in `crates/config/src/grammar/`. They are the *only* place each rule appears.

### 4.1 `Bool`

INI: `0|1|true|false` case-insensitive (analysis §7 #1). Used by `grammar::bool::parse_ini_bool(&str) -> Result<bool, ConfigError>`. TOML uses serde's native `bool`.

### 4.2 `Number`

INI: decimal-only, optional leading `+`, hard fail on overflow (analysis §7 #2). Single generic parser `grammar::number::parse_ini_int::<T: PrimInt>(&str)`. TOML uses serde's native integer parsing (which permits `0x`/`0o`/`0b`/`_`).

### 4.3 `Duration`

Per analysis §7 #3, there is **no unified `Duration` type**. The bulk of duration fields are bare integers in named units (seconds / milliseconds). Only `amendment_majority_time` has a custom grammar; it lives in `grammar/duration.rs` as a single function. INI and TOML both produce a `std::time::Duration` via this function, but TOML's grammar tightens to disallow trailing junk (analysis §7 #3 refinement).

The schema uses `u32` for plain-integer second/millisecond fields and `Duration` for `amendment_majority_time`. No `Duration` wrapper proliferation.

### 4.4 `RelPath` vs `PathBuf`

A typed distinction makes path-resolution policy visible in the schema:

```rust
/// A path that the config crate will resolve relative to the config dir
/// during `Config::bootstrap()`.
pub struct RelPath(pub PathBuf);

/// A path stored verbatim. Whoever consumes it is responsible for absolutising.
type Path = PathBuf;
```

Per analysis §7 #5:
- **INI:** the three "magic" auto-resolved fields (`debug_logfile`, `database_path`, `validators_file`) are typed `RelPath`. All other path fields are plain `PathBuf` (taken verbatim).
- **TOML strict refinement:** all relative path fields resolve uniformly. Implementation: the TOML→`Config` convert path wraps any relative `PathBuf` in a "resolve-me" marker, applied during `bootstrap()`. Concretely, every TOML path field is post-processed: if relative, it's resolved against the config dir; if absolute, it's left unchanged. The schema doesn't carry two type variants — the resolution policy is applied at bootstrap time, so by the time a getter returns the value it's already resolved (TOML) or verbatim (INI, except the three `RelPath` fields).

This keeps `Parsed` format-agnostic (its consumers don't need to know which format produced it) while preserving the asymmetric rule.

### 4.5 `HostPort`

```rust
pub struct HostPort {
    pub host: HostKind,
    pub port: Option<u16>,
}

pub enum HostKind {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Hostname(String),
}
```

Grammar (analysis §6.16):
- `host port` — space-separated, two tokens.
- `host:port` — exactly one `:` (or the host is a bracketed IPv6 literal `[fe80::1]:51235`).
- `host` alone — port = `None`.

IPv6 without port uses the space-separated form (`fe80::1 51235`) or bare `fe80::1`. The colon-rewrite collision rule from the existing C++ code (`":([0-9]+)$"` only fires on a single-colon line) is reproduced in this parser.

## 5. INI parsing pipeline

Two-stage parse (locked in by analysis §6.1).

**Stage 1: lex + section bag.** `ini::lexer::tokenize(&str) -> RawSections`.

```rust
pub struct RawSections {
    pub sections: Vec<RawSection>,
    // index for fast lookup; built once after tokenize finishes.
    by_name: HashMap<String, Vec<usize>>,
}

pub struct RawSection {
    pub name: String,
    pub lines: Vec<RawLine>,
    pub span: SourceSpan,  // for error reporting
}

pub struct RawLine {
    pub kind: RawLineKind,           // KeyValue { key, value } | BareValue(String)
    pub span: SourceSpan,
    pub had_trailing_comment: bool,  // preserved for back-compat probes
}
```

Tokenisation rules (matching analysis §1.2–§1.4 verbatim):

1. Normalize line endings (`\r\n` and `\r` → `\n`).
2. Drop blank lines.
3. Drop whole-line comments (lines whose first non-whitespace char is `#`).
4. Strip trailing `#...` comments. Honor the `\#` escape (analysis §6.10).
5. Detect `[name]` headers; lines without `]` reuse the current section.
6. For each remaining line, run the key regex `[a-zA-Z][_a-zA-Z0-9]*\s*=\s*(.+\S)`. Match → `KeyValue`. Miss → `BareValue` (including `key=` per analysis §6.11).
7. Trim trailing whitespace from values.

Two `[name]` headers in one file concatenate their lines (analysis §1.2 final paragraph). The same is true if `validators.txt` introduces a section that also exists in the main config — see §5.3.

**Stage 2: section → typed struct.** `ini::adapt::adapt(RawSections) -> Result<Config, ConfigError>` produces a `Config` whose `Parsed` bucket is fully populated.

The schema structs in `types/` are the *single source of truth*. Every struct that maps to a kv section carries `#[derive(Deserialize)]`. The same struct is consumed by both INI and TOML; the format-specific work is the deserializer, not the schema.

Stage 2 dispatches each `RawSection` into one of three handler categories:

**Category 1: pure-kv sections** — handled by a custom INI deserializer driven by the struct's `Deserialize` derive. No per-section code needed beyond the dispatch entry.

```rust
// types/overlay.rs
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]                        // missing fields → field defaults
pub struct OverlayConfig {
    pub public_ip: Option<IpAddr>,
    pub ip_limit: Option<u32>,
    pub max_unknown_time: u32,           // default via Default impl
    pub max_diverged_time: u32,
}

// ini/adapt.rs
fn dispatch(raw: &RawSection, cfg: &mut Config) -> Result<(), ConfigError> {
    match raw.name.as_str() {
        "overlay"           => cfg.parsed.overlay
                                  = ini::serde::from_kv_section(raw)?,
        "node_db"           => cfg.parsed.node_db
                                  = ini::serde::from_kv_section(raw)?,
        "sqlite"            => cfg.parsed.sqlite
                                  = ini::serde::from_kv_section(raw)?,
        "transaction_queue" => cfg.parsed.transaction_queue
                                  = ini::serde::from_kv_section(raw)?,
        // … one line per pure-kv section
        _ => {}                          // unknown section: lenient drop
    }
    Ok(())
}
```

Adding a new field to `OverlayConfig` is a single struct edit; the dispatcher is untouched. This is the central "single source of truth" win.

**Category 2: bare-line lists** — section payload is `Vec<RawLine>` of bare values, each parsed by the item type's `Deserialize` impl (typically `from_str`).

```rust
// types/hostport.rs — implements FromStr, then Deserialize from string
impl FromStr for HostPort { … }

// ini/adapt.rs
"ips"           => cfg.parsed.ips        = ini::serde::from_bare_lines(raw)?,
"ips_fixed"     => cfg.parsed.ips_fixed  = ini::serde::from_bare_lines(raw)?,
"features"      => cfg.parsed.features   = ini::serde::from_bare_lines(raw)?,
"validators"    => cfg.parsed.trusted_validators
                          .extend(ini::serde::from_bare_lines(raw)?),
"cluster_nodes" => cfg.parsed.cluster_nodes = ini::serde::from_bare_lines(raw)?,
// …
```

**Category 3: special-shape sections** — handwritten adapters. These are the dual-shape sections, the single-bare-line sections, and the dynamic `[port_<name>]` enumeration.

```rust
"server" => adapt_server(raw, &mut cfg.parsed)?,         // bare names + kv defaults
"crawl"  => cfg.parsed.crawl = adapt_crawl(raw)?,        // bool OR kv map
"database_path"    => cfg.parsed.database_path
                          = Some(RelPath(adapt_single_line(raw)?)),
"network_id"       => cfg.parsed.network_id
                          = adapt_network_id(raw)?,       // "main"/"testnet"/int
"validator_token"  => cfg.parsed.validator_token
                          = Some(adapt_multi_line_blob(raw)?),
// …
```

`[port_*]` is dispatched after the first sweep: once `[server]` is parsed, we know the port names, and each `[port_<name>]` runs through `ini::serde::from_kv_section::<PortConfig>` — Category 1 again.

### 5.1 The INI deserializer (`ini/serde.rs`)

A custom `serde::Deserializer` that views a `RawSection` as a map (for kv sections) or a seq (for bare-line sections). The interesting parts:

- `deserialize_bool` calls `grammar::bool::parse_ini_bool` (the `0|1|true|false` rule).
- `deserialize_u32` / `deserialize_i64` / etc. call `grammar::number::parse_ini_int` (decimal-only).
- `deserialize_str` returns the raw value verbatim. Types with custom grammar (`HostPort`, `MajorityDuration`, …) implement `Deserialize` via `FromStr` and own their own parsing.
- `deserialize_option` returns `None` if the key is absent in the map, `Some(visited_inner)` if present.
- Unknown keys in a map are silently consumed (lenient mode). The `MapAccess` impl never reports a key that has no matching field — it filters before yielding.

Public surface from this module:

```rust
pub fn from_kv_section<T: DeserializeOwned>(raw: &RawSection)
    -> Result<T, ConfigError>;
pub fn from_bare_lines<T: DeserializeOwned>(raw: &RawSection)
    -> Result<T, ConfigError>;
```

Size estimate: ~250 lines for the deserializer + ~50 for `MapAccess`/`SeqAccess` impls. One-time investment.

### 5.2 Lenient validation hook

Some INI fields have silent-clamp rules that derive can't express (`max_transactions` clamped to 100..1000, `fetch_depth` floored at 10, etc.). Handled with a per-section `validate_lenient(&mut self)` method, called immediately after deserialization in INI mode and *not* called in TOML mode (where out-of-range is an error):

```rust
impl OverlayConfig {
    pub(crate) fn validate_lenient(&mut self) {
        self.max_unknown_time = self.max_unknown_time.clamp(300, 1800);
        self.max_diverged_time = self.max_diverged_time.clamp(60, 900);
    }
}
```

For TOML, the same range checks live in a separate `validate_strict(&self) -> Result<(), ConfigError>` method that returns errors instead of clamping. Both methods are on the schema struct so they're easy to find next to the field they validate.

### 5.3 INI lenience semantics

- Unknown top-level sections → silently dropped at dispatch (the `match` arm falls to `_ => {}`).
- Unknown keys inside known sections → silently dropped by the deserializer's `MapAccess` (no `deny_unknown_fields` in INI mode).
- Out-of-range values for clamped fields → silently clamped by `validate_lenient`.
- Parse failures (`network_id = banana`) → still hard errors. "Lenient" means "ignores unknown stuff and clamps documented ranges"; it doesn't paper over malformed values.

Each section's Category 1/2/3 path is tested in isolation against fixtures in `crates/config/tests/fixtures/ini/`.

### 5.4 Lift fields and the death of side-channel reads

Per analysis §6.13, `Config::FAST_LOAD` and `Config::USE_TX_TABLES_` are dropped. Their canonical homes are `cfg->node_db().fast_load()` and `cfg->ledger_tx_tables().use_tx_tables()`. C++ consumers are migrated to these paths in step 4 (see §11).

### 5.5 `validators.txt` splice

Implemented in `ini::adapt::merge_validators_file` (called from `Config::bootstrap()`, not from `parse_ini` — splicing depends on knowing the config dir). Reads the secondary file, tokenises it the same way, and **merges** the allow-listed sections (`[validators]`, `[validator_keys]`, `[validator_list_sites]`, `[validator_list_keys]`, `[validator_list_threshold]`) into the running `RawSections` *before* dispatch runs. Any section in the secondary file outside the allow-list is an error in TOML, ignored in INI.

Overlap (same section in both files):
- INI: append (matches existing behavior).
- TOML: error `ValidatorsFileOverlap { section }`.

If `validators_file` is unset in the main config, look for `<config_dir>/validators.txt` and silently ignore if missing (analysis §3.5 / §7 #9).

## 6. TOML parsing pipeline

`toml::parse_toml(text) -> Result<Config, ConfigError>`.

Since the `types/` schema structs already carry `#[derive(Deserialize)]` (§5), TOML is mostly free. The TOML module contributes:

- A top-level `Root` struct in `toml/schema.rs` with `#[serde(deny_unknown_fields)]` and one field per section, mirroring the canonical TOML layout (flat tables for everything except `[port.<name>]`, which is table-of-tables per analysis §7 #6 — see §3.1).
- Strict-mode wrappers / attributes on the schema structs that diverge from INI:
  - `#[serde(deny_unknown_fields)]` propagated to every nested struct.
  - The `validate_strict()` method on each struct (introduced in §5.2) replaces `validate_lenient()` — same fields, error instead of clamp.
  - The `amendment_majority_time` deserializer uses the strict grammar (no trailing junk).
  - Path resolution applied during `From<Root> for Config`, per §4.4.

```rust
// toml/schema.rs
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Root {
    // Top-level scalars (formerly single-bare-line INI sections; see §7)
    #[serde(default)] pub network_id: u32,
    #[serde(default)] pub network_quorum: u64,
    #[serde(default)] pub peer_private: bool,
    pub database_path: Option<PathBuf>,
    pub debug_logfile: Option<PathBuf>,
    pub validators_file: Option<PathBuf>,
    pub node_size: Option<NodeSize>,
    pub validation_seed: Option<String>,
    pub validator_token: Option<String>,
    // … one field per top-level scalar

    // Top-level arrays (formerly bare-line list sections)
    #[serde(default)] pub ips: Vec<HostPort>,
    #[serde(default)] pub ips_fixed: Vec<HostPort>,
    #[serde(default)] pub features: HashSet<FeatureName>,
    #[serde(default)] pub validators: Vec<TrustedValidator>,
    #[serde(default)] pub validator_list_sites: Vec<String>,
    #[serde(default)] pub validator_list_keys: Vec<String>,
    #[serde(default)] pub sntp_servers: Vec<String>,
    #[serde(default)] pub cluster_nodes: Vec<ClusterNode>,
    #[serde(default)] pub amendments: Vec<KnownAmendment>,
    #[serde(default)] pub veto_amendments: Vec<KnownAmendment>,
    // …

    // Tables (formerly multi-key INI sections)
    #[serde(default)] pub server: ServerConfig,
    #[serde(default)] pub port: BTreeMap<String, PortConfig>,
    #[serde(default)] pub node_db: NodeDbConfig,
    pub import_db: Option<NodeDbConfig>,
    #[serde(default)] pub sqlite: SqliteConfig,
    #[serde(default)] pub overlay: OverlayConfig,
    #[serde(default)] pub reduce_relay: ReduceRelayConfig,
    #[serde(default)] pub crawl: CrawlConfig,
    #[serde(default)] pub voting: VotingConfig,
    #[serde(default)] pub transaction_queue: TxQConfig,
    #[serde(default)] pub insight: InsightConfig,
    #[serde(default)] pub perf: PerfConfig,
    #[serde(default)] pub ledger_tx_tables: LedgerTxTablesConfig,
    // …
}

pub fn parse_toml(text: &str) -> Result<Config, ConfigError> {
    let root: Root = ::toml::from_str(text).map_err(...)?;
    let cfg = Config::from(root);          // runs validate_strict() per nested struct
    Ok(cfg)
}
```

Field names in `Root` match the field names on `Parsed` 1:1, so `From<Root> for Config` is largely a `Config { parsed: Parsed { network_id: root.network_id, ips: root.ips, overlay: root.overlay, … }, .. }` move — no renaming, no remapping.

Because the schema is shared with INI, adding a field to `OverlayConfig` (or any other section struct) automatically picks up on the TOML side — the same single-edit win that motivated §5's serde approach.

`validators_file` splice in TOML mode: the secondary file is parsed independently into a `Config`, then merged at the field level (not the raw level). Merging means concatenating the validator-list collections on `Parsed`. Overlap is an error.

## 7. INI ↔ TOML equivalence rules

The `--convert-config` tool (step 4) needs a deterministic mapping. The principle: bare-line sections become idiomatic top-level TOML — scalars or arrays — not nested under fake section keys. Multi-key sections stay as TOML tables.

| INI source | TOML target |
|---|---|
| `[overlay]` with kv pairs | `[overlay]` table with the same keys |
| `[node_db]`, `[sqlite]`, `[transaction_queue]`, … | same — table with kv keys |
| `[port_<name>]` | `[port.<name>]` |
| `[server]` (mixed bare names + kv defaults) | `[server]` table with `ports = [...]` plus the kv keys |
| `[ips]`, `[ips_fixed]`, `[sntp_servers]`, `[cluster_nodes]`, `[amendments]`, `[veto_amendments]`, `[rpc_startup]` (bare-line lists) | top-level array: `ips = ["host1:port1", "host2:port2"]` |
| `[features]` (bare list) | top-level array: `features = ["Flow", "TickSize", …]` |
| `[validators]`, `[validator_keys]` (bare keys + optional label) | top-level array of inline tables: `validators = [{ key = "n...", label = "Alice" }, …]` |
| `[validator_list_sites]`, `[validator_list_keys]` | top-level arrays of strings |
| `[validator_list_threshold]` single value | top-level scalar: `validator_list_threshold = 3` |
| `[database_path]`, `[debug_logfile]`, `[validators_file]`, `[node_size]`, `[network_id]`, `[network_quorum]`, `[peer_private]`, `[peers_max]`, …  (single-bare-line) | top-level scalar with the same field name: `database_path = "/var/lib/xrpld"`, `network_id = 1`, … |
| `[validator_token]`, `[validation_seed]`, `[validator_key_revocation]` (multi-line blobs) | top-level scalar string (multi-line `"""..."""` for the token): `validator_token = """..."""` |
| `[crawl] true` (LegacyBool) | `[crawl]` table with all four flags set true (LegacyBool unrolls; `--convert-config` warns) |
| trailing `#` comments | dropped (TOML supports comments but the converter doesn't preserve them) |

Note: the TOML field names match the INI section names 1:1 (lowercase, underscores). No renaming. This means the TOML `Root` struct in `toml/schema.rs` has the same field names as the `Parsed` struct, and converting `Parsed → Root` is mostly a structural rename of containers (Vec stays Vec, scalar stays scalar) rather than a key remapping.

Converter ordering is irrelevant to runtime correctness — both formats are parsed into the same `Config` — but the converter should preserve source order where it can, for diff-friendliness.

`--convert-config` runs the INI pipeline against the source file, then *renders* the resulting `Config` into a canonical TOML document. Rendering is the inverse of the TOML `From` impl. Validation runs before rendering, so the converter doubles as a `--check-config` for INI.

## 8. `NodeSize`, `SizedItem`, `kSIZED_ITEMS`

Per analysis §7 #8, the table moves to Rust verbatim.

```rust
// crates/config/src/schema/sized.rs

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NodeSize { Tiny = 0, Small, Medium, Large, Huge }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SizedItem {
    SweepInterval = 0,
    AccountIdCacheSize,
    LedgerSize,
    TreeCacheSize,
    NodeCacheSize,
    // … 13 entries total, matching kSIZED_ITEMS verbatim
}

const SIZED_TABLE: [[i32; 5]; 13] = [
    /* SweepInterval */     [10, 60, 120, 300, 600],
    /* AccountIdCacheSize */ [ … ],
    // … values copied verbatim from src/xrpld/core/detail/Config.cpp:114-137
];

pub const fn sized_value(item: SizedItem, size: NodeSize) -> i32 {
    SIZED_TABLE[item as usize][size as usize]
}
```

Auto-detection (analysis §2.4):

```rust
// crates/config/src/bootstrap.rs
pub fn detect_node_size() -> NodeSize {
    let ram_gb = probe_ram_gb();              // sysctl HW_MEMSIZE / sysinfo / GMSE
    let cpu_cap = available_parallelism().get() / 2;
    let by_ram = walk_ram_buckets(ram_gb);    // first row of SIZED_TABLE that fits
    NodeSize::cap(by_ram, NodeSize::from_index(cpu_cap))
}
```

`probe_ram_gb` is the cross-platform RAM probe — hand-rolled `cfg(target_os = "…")` blocks calling the same syscalls the C++ code does today (macOS `sysctl HW_MEMSIZE`, Linux `sysinfo(2)`, Windows `GlobalMemoryStatusEx`). ~50 lines, zero new dependencies. See §15 Q1 for the rationale (`sysinfo` crate was the alternative).

`Config::bootstrap()` runs detection if `parsed.node_size.is_none()`. Test rigs that need a deterministic size call a `Config::set_detected_node_size(NodeSize::Tiny)` override before bootstrap to bypass the probe.

`Config::getValueFor(item, optional<node>)` on the C++ side becomes `Config::sized_value(item)` (current effective node size) and `Config::sized_value_for(item, node)` (explicit override). ~16 call sites convert to FFI calls returning a primitive `int`. The shape question (flat function vs. per-`SizedItem` typed method) is deferred to implementation; see §15 Q2.

## 9. Bootstrap (`crates/config/src/bootstrap.rs`)

Everything in analysis §4 that the analysis assigned to Rust lives here. Public entrypoints (most are called *through* `Config`, not directly):

```rust
pub fn discover_config_file(explicit: Option<PathBuf>, sys_name: &str)
    -> Result<PathBuf, ConfigError>;
pub fn detect_node_size() -> NodeSize;
pub fn ensure_data_dir(path: &Path, standalone: bool) -> Result<(), ConfigError>;
pub fn splice_validators_file(cfg: &mut Config, path: &Path) -> Result<(), ConfigError>;
```

`Config::bootstrap()` (defined in `config.rs`, delegates to helpers here) does the following in order:

1. Resolve `config_dir` from the path used at `from_file` time (or, for `from_*_str`, panic — pure-string callers must provide it via an explicit `set_config_dir` setter; see §15 Q7).
2. Resolve the three auto-resolved `RelPath` fields (`debug_logfile`, `database_path`, `validators_file`) against `config_dir`.
3. If `validators_file` is set, splice the referenced file. If unset, try `<config_dir>/validators.txt` and silently ignore if missing (matches existing behavior).
4. Run cross-section validators (analysis §5 — `network_quorum ≤ effective peers_max`, `online_delete ≥ ledger_history`, `checkZeroPorts`, the `validation_seed`/`validator_token` XOR, etc.).
5. Resolve `data_dir`: `[database_path]` if set else `config_dir/db`. `mkdir -p` unless `standalone()`.
6. Determine `node_size_effective`: `parsed.node_size` if set, else `detect_node_size()`. Forced to `Tiny` if `standalone()`.
7. Emit `stderr` echo of the loaded config path unless `quiet()`.
8. Write everything into `self.finalized = Some(Finalized { … })`.

The order matches the existing C++ code so the step-4 migration doesn't accidentally reorder a side effect.

**Two deliberate deviations from the C++ flow:**

- In C++, `LEDGER_HISTORY = 0` (under standalone) and `path_search_max = 0` (under validator identity) are applied by mutating the parsed config in place. Here `Parsed` is not mutated — the getters compute effective values on each call:

  ```rust
  impl Config {
      pub fn ledger_history(&self) -> LedgerHistory {
          if self.standalone() { LedgerHistory::None_ }
          else { self.parsed.ledger_history }
      }
      pub fn path_search_max(&self) -> i32 {
          if self.parsed.validation_seed.is_some()
             || self.parsed.validator_token.is_some() { 0 }
          else { self.parsed.path_search_max }
      }
  }
  ```

- In C++, `[fee_default]` overwrites `[voting].reference_fee` in place. Here the override stays separate (`parsed.fee_default: Option<u64>`) and the `voting()` getter merges:

  ```rust
  impl Config {
      pub fn voting(&self) -> VotingConfig {
          let mut v = self.parsed.voting.clone();
          if let Some(f) = self.parsed.fee_default { v.reference_fee = f; }
          v
      }
  }
  ```

  (Note: `voting()` returns by value here because of the merge — slight cost; the alternative is to materialize the merged value into `Finalized` once. Negligible either way, deferred to impl.)

This is what analysis §7 #13 means by "resolved-path views live on the runtime layer".

## 10. FFI surface (cxx-rs)

`crates/config/src/ffi.rs` declares the bridge. The current placeholder at [crates/config/src/lib.rs:1](crates/config/src/lib.rs) gets replaced with one section per opaque type the C++ side needs.

### Design constraints

- `cxx::bridge` doesn't support `Option<T>`, generic enums, or `BTreeMap`. Everything that crosses the bridge is either: a primitive (`i32`/`u64`/`bool`), a borrowed `&str` (`rust::Str` on the C++ side — zero-copy view), an owned `String` (only when the C++ caller must outlive the Rust value), a `cxx::Vec<T>` of bridge-compatible `T`, or an opaque `&Config` / `&PortConfigHandle` / etc. with accessor methods.
- All Rust types crossing the bridge expose their data through accessor methods, not field projection. This makes type evolution painless: adding a field to `PortConfig` doesn't break the C++ side.
- **Errors:** Rust functions that can fail return a wrapped outcome opaque (`ConfigOutcome` / `UnitOutcome`), not `Result<T>`. The opaque has `has_value()` / `has_error()` / `value()` / `error()` / `into_value()` accessors. No exceptions ever cross the bridge. See "Error reporting" below.
- **Allocation discipline:** scalar string getters return `&str` (zero-copy view of a Rust-owned buffer) wherever the Rust value lives for the lifetime of `Config`. Sub-struct handles are stored on `Config` at bootstrap and returned by `&Handle` (zero-copy borrow) rather than `Box<Handle>` (heap-allocated owned). Collections (`Vec<HostPortFfi>`, …) stay owned — they're read once at startup and the copy cost is negligible.

### Bridge module sketch

```rust
#[cxx::bridge(namespace = "rs::config")]
mod ffi {
    // Plain-data FFI types ("shared structs" — owned by either side)
    pub struct HostPortFfi { host: String, port: u16, has_port: bool }
    pub struct NodeIdFfi   { key: String, label: String, has_label: bool }

    // The single opaque handle.
    extern "Rust" {
        type Config;

        // -------- constructors --------
        fn load(path: &CxxString) -> Result<Box<Config>>;
        fn parse_ini(text: &CxxString) -> Result<Box<Config>>;
        fn parse_toml(text: &CxxString) -> Result<Box<Config>>;

        // -------- override setters --------
        fn set_quiet(self: &mut Config, v: bool);
        fn set_silent(self: &mut Config, v: bool);
        fn set_standalone(self: &mut Config, v: bool);
        fn set_start_ledger(self: &mut Config, v: &CxxString);
        fn set_validation_quorum(self: &mut Config, v: u64);
        fn set_force_multi_thread(self: &mut Config, v: bool);
        // … one method per CLI field on the Rust side

        // -------- finalize --------
        fn bootstrap(self: &mut Config) -> Result<()>;

        // -------- scalar getters (primitives by value) --------
        fn network_id(self: &Config) -> u32;
        fn network_quorum(self: &Config) -> u64;
        fn peer_private(self: &Config) -> bool;
        // … one method per top-level field consumed by C++

        // sentinel-encoded enums
        fn ledger_history(self: &Config) -> i64;   // -1=None, MAX=Full, else count
        fn fetch_depth(self: &Config) -> i64;
        fn sized_value(self: &Config, item: u8) -> i32;
        fn sized_value_for(self: &Config, item: u8, node: u8) -> i32;

        // -------- borrowed strings (zero-copy &str views) --------
        // empty = unset (paths can't be empty in practice; consider pairing
        // with a has_X() predicate if ambiguous)
        fn debug_logfile<'a>(self: &'a Config) -> &'a str;
        fn database_path<'a>(self: &'a Config) -> &'a str;
        fn data_dir<'a>(self: &'a Config) -> &'a str;
        fn config_dir<'a>(self: &'a Config) -> &'a str;
        fn server_domain<'a>(self: &'a Config) -> &'a str;
        fn validation_seed<'a>(self: &'a Config) -> &'a str;

        // -------- collections (owned, read once at startup) --------
        fn ips(self: &Config) -> Vec<HostPortFfi>;
        fn ips_fixed(self: &Config) -> Vec<HostPortFfi>;
        fn cluster_nodes(self: &Config) -> Vec<NodeIdFfi>;
        fn features(self: &Config) -> Vec<String>;
        fn port_names(self: &Config) -> Vec<String>;

        // -------- sub-struct handles (borrowed; stored on Config) --------
        fn port<'a>(self: &'a Config, name: &CxxString) -> Result<&'a PortConfigHandle>;
        fn node_db<'a>(self: &'a Config) -> &'a NodeDbHandle;
        fn sqlite<'a>(self: &'a Config) -> &'a SqliteHandle;
        fn overlay<'a>(self: &'a Config) -> &'a OverlayHandle;
        // … etc.
    }

    extern "Rust" {
        type PortConfigHandle;
        fn port(self: &PortConfigHandle) -> u16;
        fn ip<'a>(self: &'a PortConfigHandle) -> &'a str;       // empty = unset
        fn protocols(self: &PortConfigHandle) -> Vec<u8>;       // PortProtocol as u8
        // … all the per-port fields the C++ side needs
    }

    extern "Rust" {
        type NodeDbHandle;
        fn kind(self: &NodeDbHandle) -> u8;
        fn path<'a>(self: &'a NodeDbHandle) -> &'a str;
        fn fast_load(self: &NodeDbHandle) -> bool;
        fn earliest_seq(self: &NodeDbHandle) -> u32;
        fn online_delete(self: &NodeDbHandle) -> i64;           // -1 = unset
        fn extra<'a>(self: &'a NodeDbHandle, key: &CxxString) -> &'a str;
        fn extra_keys(self: &NodeDbHandle) -> Vec<String>;
        // …
    }

    // (similar handle types for SqliteHandle, OverlayHandle, TxQHandle, …)
}
```

`Box<Config>` and `Box<ConfigOutcome>` / `Box<UnitOutcome>` are the only owned handles the C++ side holds; everything else accessed through them is a borrow into Rust-owned data. C++ consumes these types directly (no shim — see §11).

### Ownership and lifetimes

- Rust owns the one `Config` value. C++ holds `rust::Box<Config>` and never frees Rust memory directly — `Box`'s destructor calls back into Rust on drop.
- Sub-struct handles (`PortConfigHandle`, `NodeDbHandle`, `SqliteHandle`, …) are **stored as fields on `Config`**, materialized once during `bootstrap()`. Accessors return `&Handle` borrows tied to `&self`'s lifetime — zero allocation per call.
- C++ receives these as `rust::Reference<Handle>` (a `Handle const*` wrapper). The borrow is valid as long as the `Box<Config>` lives. In practice `Config` is a startup-time singleton, so the borrows last for the process lifetime.
- Borrowed `&str` values (paths, names, etc.) are likewise tied to `&self`. C++ receives `rust::Str` (a `{const char*, size_t}` view); callers either use it as `std::string_view` directly or copy into `std::string` if they need ownership.
- A `Config` is constructed once at startup and never replaced. No interior mutability after `bootstrap()`; getters take `&self` and don't need an `Arc` clone.

### Error reporting

No exceptions cross the bridge. Fallible Rust functions return an opaque "outcome" type that wraps the underlying `Result` internally and exposes `Expected`-flavored accessors:

```rust
// crates/config/src/ffi.rs

pub struct ConfigOutcome(Result<Box<Config>, ConfigError>);
pub struct UnitOutcome(Result<(), ConfigError>);

impl ConfigOutcome {
    fn has_value(&self) -> bool { self.0.is_ok() }
    fn has_error(&self) -> bool { self.0.is_err() }
    fn value(&self) -> &Config {
        self.0.as_ref().expect("ConfigOutcome: no value")
    }
    fn error(&self) -> &str {
        self.0.as_ref().err().map(ConfigError::message).unwrap_or("")
    }
    fn into_value(self: Box<Self>) -> Box<Config> {
        self.0.expect("ConfigOutcome: cannot unwrap error")
    }
}

impl UnitOutcome {
    fn has_error(&self) -> bool { self.0.is_err() }
    fn error(&self) -> &str { /* same shape */ }
}

impl From<Result<Box<Config>, ConfigError>> for ConfigOutcome {
    fn from(r: Result<Box<Config>, ConfigError>) -> Self { Self(r) }
}
```

C++ consumes them directly with no exception machinery:

```cpp
auto outcome = rs::config::load(rust::String{path});
if (outcome->has_error()) {
    log(outcome->error());
    return;
}
auto cfg = outcome->into_value();      // rust::Box<Config>
```

Happy path: one heap allocation for `Box<ConfigOutcome>`, accessor calls are inlined. Error path: same allocation, no placeholder Config — the opaque holds the real `Result` and the error variant carries only the message string.

**Lookup-style failures** (e.g. asking for a port name that doesn't exist) use a two-call predicate-plus-getter pattern instead of an outcome wrapper, because cxx-rs opaque types are `'static` and can't carry a borrow:

```rust
fn has_port(self: &Config, name: &CxxString) -> bool;
fn port<'a>(self: &'a Config, name: &CxxString) -> &'a PortConfigHandle;  // asserts
```

Callers check `has_port` first; calling `port` on an unknown name aborts. This pattern is analogous to `std::map::at` paired with `contains`.

**Structured error fields.** For callers (mainly `--check-config`, see §15 Q5) that need the structured fields of `ConfigError` (kind, span, source file — see §12) rather than just a string, a separate `load_detailed` entrypoint returns a richer opaque (`DetailedOutcome`) with accessors for each field. The regular load path uses the plain `ConfigOutcome` and just needs "did it fail, and what does the user see".

## 11. C++ consumption (no shim)

There is no C++ shim layer. C++ code includes the cxx-generated header from `rs_config_cxxbridge` and consumes Rust types directly. Step 4's migration work is mechanical accessor renames (`cfg.NETWORK_ID()` → `cfg->network_id()`) rather than wrapping behind a translation class.

The trade-off is explicit: a shim would let call sites migrate one at a time while the old `Config::NETWORK_ID()` surface kept working. Without a shim, step 4 has to update every call site in one sweep — but the resulting code references the Rust types directly, with no intermediate C++ class to maintain or evolve in lock-step.

### Typical consumption

```cpp
#include "rs/config/lib.rs.h"        // cxx-generated header
using rs::config::Config;

// startup
auto outcome = rs::config::load(rust::String{path});
if (outcome->has_error()) {
    std::cerr << "config load failed: " << outcome->error() << "\n";
    return 1;
}
rust::Box<Config> cfg = outcome->into_value();

cfg->set_quiet(args.quiet);
cfg->set_standalone(args.standalone);
if (args.start_ledger) cfg->set_start_ledger(rust::String{*args.start_ledger});

auto boot = cfg->bootstrap();
if (boot->has_error()) {
    std::cerr << "bootstrap failed: " << boot->error() << "\n";
    return 1;
}
```

```cpp
// runtime accessors — direct Rust calls, no wrapper class
auto netId      = cfg->network_id();
auto dataDir    = std::string{cfg->data_dir()};       // rust::Str → std::string

auto const& db  = cfg->node_db();                     // rust::Reference<NodeDbHandle>
if (db.fast_load()) { … }

if (cfg->has_port(rust::Str{"rpc_admin"})) {
    auto const& p = cfg->port(rust::Str{"rpc_admin"});
    auto portNum  = p.port();
}

int treeCache = cfg->sized_value(static_cast<std::uint8_t>(SizedItem::TreeCacheSize));
```

### Naming convention shift

The Rust API uses `snake_case`; the existing C++ uses `SCREAMING_SNAKE_CASE` for flat fields and `camelCase` for methods. Migration in step 4 normalizes everything to the Rust snake_case via the cxx-generated header. So:

| Old C++ | New C++ |
|---|---|
| `cfg.NETWORK_ID()` | `cfg->network_id()` |
| `cfg.PEER_PRIVATE()` | `cfg->peer_private()` |
| `cfg.useTxTables()` | `cfg->ledger_tx_tables().use_tx_tables()` |
| `cfg.FAST_LOAD` | `cfg->node_db().fast_load()` |
| `cfg.getValueFor(item)` | `cfg->sized_value(static_cast<std::uint8_t>(item))` |
| `cfg.getDebugLogFile()` | `std::string{cfg->debug_logfile()}` |
| `cfg.section("foo").get<T>("k", default)` | `cfg->foo().k()` |

Step-4 migration is mostly find-and-replace at this point. The retired `Section::get<T>` / `legacy()` accessors disappear — there's no compatibility surface.

### Tests

The ~150 unit tests that today use `Config::loadFromString(text)` migrate to:

```cpp
auto outcome = rs::config::parse_ini(rust::Str{text});
ASSERT_FALSE(outcome->has_error());
auto cfg = outcome->into_value();
cfg->bootstrap();   // (or assert if a test deliberately probes pre-bootstrap state)
```

A small header `test/jtx/config_test_helpers.h` can wrap this in a one-call `auto cfg = makeConfig(text);` helper for terseness — not a shim, just a test convenience. Tests that today set fields directly (`cfg.FORCE_MULTI_THREAD = true`) become `cfg->set_force_multi_thread(true)`.

### Lifetime discipline

`rust::Box<Config>` is the owning handle; conventionally stored on the `Application` (or its tests' analog) by value. All `rust::Reference<…Handle>` borrows and `rust::Str` views obtained from it remain valid for the application's lifetime. Sub-handles must not outlive the owning `Box<Config>` — the compiler can't enforce this across cxx, so the codebase convention is "never store a `rust::Reference` long-term; re-fetch it from the `Config` when needed". Cheap by construction (no allocation).

## 12. Error handling

```rust
// crates/config/src/error.rs

#[derive(Debug, thiserror::Error)]
pub struct ConfigError {
    pub kind: ConfigErrorKind,
    pub span: Option<SourceSpan>,
    pub source_file: Option<PathBuf>,
}

pub enum ConfigErrorKind {
    Lex { reason: LexError },
    UnknownSection { name: String, format: Format },     // TOML strict only
    UnknownKey { section: String, key: String, suggestion: Option<String> },
    Grammar { what: &'static str, value: String, reason: String },
    OutOfRange { field: String, value: i64, min: Option<i64>, max: Option<i64> },
    MutualExclusion { first: String, second: String },
    OrphanPortTable { name: String },
    ValidatorsFileOverlap { section: String },
    Cross { what: String },                              // catch-all for §5 validators
    Io { path: PathBuf, source: std::io::Error },
}

pub struct SourceSpan {
    pub line: u32,
    pub col_start: u32,
    pub col_end: u32,
}

pub enum Format { Ini, Toml }
```

`Display` produces messages of the form:

```
config error at /etc/opt/rippled/rippled.cfg:42:5: unknown key `foobar` in section [sqlite]
  = note: did you mean `temp_store`?
```

"Did-you-mean" uses a Levenshtein cutoff (e.g. distance ≤ 2) over the known-key set. Cheap to compute, big UX win in strict TOML mode. Skipped for INI (lenient mode silently ignores unknown keys, so there's no error to attach a suggestion to).

Errors are short-circuiting at the section level: the first error in a section terminates that section's adapter, but other sections still parse. The final error reported to the user is the *first* error in source order. (Aggregating multiple errors is feasible but not in scope for step 3.)

## 13. Test strategy

Two layers, following standard Cargo conventions.

### 13.1 Unit tests — inline in each source file

Every `.rs` module that contains non-trivial logic carries its own `#[cfg(test)] mod tests` at the bottom. These exercise the module's internals (including `pub(crate)` and private items, which aren't visible to integration tests) and run on every `cargo test`.

```rust
// crates/config/src/grammar/bool.rs
pub fn parse_ini_bool(s: &str) -> Result<bool, ConfigError> { … }

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn accepts_canonical_forms() {
        assert!(parse_ini_bool("0").is_ok_and(|v| !v));
        assert!(parse_ini_bool("1").is_ok_and(|v|  v));
        assert!(parse_ini_bool("true").unwrap());
        assert!(parse_ini_bool("FALSE").is_ok_and(|v| !v));   // case-insensitive
    }
    #[test] fn rejects_yes_no() {
        assert!(parse_ini_bool("yes").is_err());
        assert!(parse_ini_bool("no").is_err());
    }
}
```

Coverage targets by module:

- `grammar/{bool,number,duration,hostport,path}.rs` — every accepted/rejected form, including overflow / out-of-range.
- `ini/lexer.rs` — comment stripping, `\#` escape, section-header collisions, line-ending normalisation, `key=` (empty value) behavior.
- `ini/serde.rs` — the deserializer's `MapAccess`/`SeqAccess` impls; unknown-key handling in lenient mode.
- `ini/adapt.rs` — each Category-3 special-shape adapter (`adapt_server`, `adapt_crawl`, single-bare-line adapters).
- `toml/schema.rs` — `From<Root> for Config` and the cross-field reconciliation (port names ↔ port tables).
- `types/*` — `validate_lenient` / `validate_strict` for each section struct.
- `bootstrap.rs` — `discover_config_file` with synthetic env vars; `detect_node_size` with mocked inputs.
- `error.rs` — `Display` output formatting, did-you-mean suggestions.

These tests are pure-Rust and fast (no I/O beyond temp files where strictly needed).

### 13.2 Integration tests — `crates/config/tests/`

Cross-module behavior, fixture-driven. Each `.rs` file under `tests/` compiles to a separate test binary and sees only the public API of the crate.

```
crates/config/tests/
├── ini_fixtures.rs         # parameterised: load every fixtures/ini/*.cfg
├── toml_fixtures.rs        # parameterised: load every fixtures/toml/*.toml
├── format_equivalence.rs   # pair-wise INI vs TOML round-trip
├── example_config.rs       # cfg/xrpld-example.cfg parses + matches snapshot
├── validators_splice.rs    # validators.txt splice scenarios
├── strict_errors.rs        # TOML inputs that should error, asserted against
│                           # expected error kind + span
└── fixtures/
    ├── ini/                # small focused INI files (one section group each)
    ├── toml/               # TOML siblings of ini/ for equivalence runs
    ├── regression/         # full-config inputs (xrpld-example.cfg copy, etc.)
    └── strict_errors/      # malformed/strict-mode-failing TOML
```

Test categories:

- **Fixture parse + snapshot.** Load fixture, call `parse_ini`/`parse_toml`, render the resulting `Config` to a stable text form (via a debug serializer or `insta`-style snapshot), compare against checked-in expected output. Catches schema drift.
- **Cross-format equivalence.** For each INI fixture with a TOML sibling, assert both produce identical `Config` values (modulo the documented format-asymmetric exceptions like `max_transactions` clamping).
- **Regression: `xrpld-example.cfg`.** The canonical example config must parse without error and match a checked-in snapshot.
- **Validators-file splice.** Multi-file fixtures exercise main-config + `validators.txt` combinations: present/absent, overlap (TOML errors / INI silent-appends), unknown sections in the secondary file.
- **Strict-error fixtures.** Minimal TOML inputs that *should* error, paired with the expected `ConfigErrorKind` and `SourceSpan`. Catches regressions in error messages — important because operators read these.
- **Fuzz target.** Opt-in `cargo fuzz` target on `parse_ini` and `parse_toml`. Goal: no panics, no hangs. Run in CI on demand, not on every PR.

### 13.3 C++-side tests

Out of scope for step 3. After step 4 lands, C++ integration tests feed a fixture `rippled.cfg` through `rs::config::load` directly and assert accessor outputs match the pre-rewrite behavior. The Rust crate is testable in isolation without C++; the schema being usable from both sides is the requirement, not joint testing.

## 14. Build & toolchain

Already integrated; this section is a checklist, not a plan.

- Workspace at `crates/`, member `crates/config/`. ✅ (existing)
- `cxx = "1.0.194"` workspace dep. ✅ (existing)
- Corrosion-based CMake import: [crates/CMakeLists.txt:14](crates/CMakeLists.txt). ✅ (existing)
- `add_xrpl_crate(rs_config CRATE config FILES lib.rs)` wired. ✅ (existing)
- Conan: no new dependencies. Rust's stdlib + a small set of crates (`serde`, `toml`, `regex`, `thiserror`, `sysinfo`-or-equivalent, `cxx`). Versions pinned in `crates/Cargo.lock`.
- MSRV: matches the toolchain currently selected by Corrosion (will be pinned in `rust-toolchain.toml` during step 3).
- Static linking: `crate-type = ["staticlib", "rlib"]`. ✅ (existing)

New dependencies the design pulls in (subject to §15 Q1):

- `serde` + `toml` (TOML parsing). Pretty much unavoidable.
- `regex` (one regex for `amendment_majority_time`, one for the colon-rewrite collision detector). Could be replaced by handwritten parsing if minimising deps matters.
- `thiserror` (ergonomics — could be omitted at the cost of `impl Error` boilerplate).
- `serde_json` (rpc_startup commands; already a JSON-flavored field today).
- A RAM/CPU probe (`sysinfo` or handwritten per-OS code).

## 15. Resolved decisions

The seven items below were open during design review. All have been resolved; this section is kept as a decision log so step 3 implementers can see which alternatives were considered and why each was rejected.

### Q1. RAM/CPU probe — `sysinfo` crate vs. handwritten per-OS code

`detect_node_size` needs RAM (GiB) and `available_parallelism()`. `available_parallelism` is in `std`; the RAM probe isn't. Options:

- **`sysinfo` crate** — mature, multi-platform, but pulls a bunch of extra info we don't need. Build cost: ~3-5 transitive deps.
- **Handwritten `#[cfg(target_os = "…")]`** — three platforms (macOS `sysctl HW_MEMSIZE`, Linux `sysinfo(2)`, Windows `GlobalMemoryStatusEx`), ~50 lines total. Mirrors what the C++ does today.

**Decision: handwritten.** Three syscalls, well-trodden ground, zero dependency surface.

### Q2. Shape of `sized_value` across FFI

The Rust API is `sized_value(item: SizedItem, size: NodeSize) -> i32`. Two shapes for the C++ side:

- **(a)** Two FFI methods on `Config`: `sized_value(item)` (current effective node size) and `sized_value_for(item, node)` (explicit override). Matches today's `Config::getValueFor` shape. ~16 call sites change uniformly.
- **(b)** Per-item typed methods: `sweep_interval_for(size)`, `tree_cache_size_for(size)`, etc. Type-safe at the call site but multiplies the FFI surface 13×.

**Decision: (a).** The table is data, not API; per-item methods would just be noise.

### Q3. `node_db.backend_extras`

Several RocksDB-specific keys (`cache_mb`, `filter_bits`, …) are passed through to `NodeStore` without Config-level interpretation. Design:

- **Lenient INI:** any unknown key under `[node_db]` falls into `backend_extras` (`BTreeMap<String, String>`).
- **Strict TOML:** only an explicit `[node_db.extras]` sub-table populates `backend_extras`. Bare unknown keys at the `[node_db]` level are errors.

This means TOML strict-mode tightens a real INI hole (`[node_db] random_typo = 5` silently ignored today).

**Decision: as designed.** Escape hatch with explicit naming in TOML. Tightening the hole is a feature of the TOML strict mode, not a regression.

### Q4. Error aggregation — first-error vs. multi-error reports

`--check-config` is much more useful if it reports every error in a file, not just the first. Same for strict-mode TOML errors. But aggregating errors complicates the adapter contract: each adapter has to return `Vec<ConfigError>` rather than `Result<_, _>`.

Options:

- **First-error** (proposed) — fewer types, simpler code; `--check-config` runs the parser repeatedly to find the next one after each fix.
- **Multi-error** — more work in step 3, materially better UX for `--check-config`.
- **Hybrid** — single error in the FFI outcome path, multi-error available behind a dedicated `parse_for_diagnostics` Rust entrypoint used only by `--check-config`.

**Decision: first-error.** Aggregation deferred — `--check-config` may grow it later if real operator UX demands it, but the parser doesn't carry the extra machinery from day one.

### Q5. Where does `--check-config` live and what does it report?

Step 4 of the plan doc commits to shipping `--check-config` and `--convert-config`. Two implementation shapes:

- **Inside `rippled`** — argv parsed by `Main.cpp`, calls into the Rust crate via FFI, prints results.
- **A separate Rust binary** in `crates/config/src/bin/` shipped alongside `rippled`.

**Decision: inside `rippled`.** One binary, one install target, one place to keep CLI argument documentation. The Rust crate exposes a `format::report(...)` helper that the C++ side invokes after `Main.cpp` parses `--check-config` / `--convert-config` from argv.

### Q6. Test helper for inline configs

With the no-shim decision (§11), the ~150 unit tests that today call `Config::loadFromString(blob)` will migrate to `rs::config::parse_ini(rust::Str{blob})->into_value()` + `cfg->bootstrap()`. That's verbose enough to make a small helper worthwhile:

- **(a)** Provide `test::makeConfig(std::string_view blob)` in `test/jtx/config_test_helpers.h` that does the unwrap + bootstrap. Tests become `auto cfg = test::makeConfig(blob);` — about the same length as the original `loadFromString` call.
- **(b)** No helper. Each test site spells out the three-line sequence.

**Decision: (a).** Pure test-side convenience, not a shim — it doesn't introduce a C++ Config class, just a function. Tests get to keep their single-call ergonomics.

### Q7. `from_ini_str` / `from_toml_str` and `config_dir`

`from_file` knows the config directory (it's `path.parent()`). The string-blob constructors don't, but `bootstrap()` needs a config dir to resolve `database_path`, `validators_file`, `debug_logfile`, and to splice `validators.txt`. Options:

- **(a)** Add a `set_config_dir(p)` setter. `bootstrap()` returns a `UnitOutcome` with `has_error() == true` and a clear `"config_dir not set; call set_config_dir before bootstrap"` message if it wasn't called. Caller-friendly; explicit failure surfaces through the normal outcome channel (no panic).
- **(b)** `from_*_str` takes `config_dir: &Path` as a second argument. No setter needed; impossible to forget. Awkward when the caller is a unit test that doesn't care about paths.
- **(c)** When called without an explicit `config_dir`, `bootstrap()` uses `std::env::current_dir()`. Matches the C++ "current directory" fallback today. Surprising for tests that happen to chdir.

**Decision: (a).** Add `set_config_dir(p)`. `bootstrap()` returns a `UnitOutcome` with `has_error()` if `config_dir` wasn't set. Unit tests that just parse skip bootstrap; tests that exercise bootstrap pass an explicit (typically temp) directory via the setter.

---

All §15 items resolved. Step 3 (Rust implementation) can begin against this contract.

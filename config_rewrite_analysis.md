# Config Rewrite — Step 1 Analysis

Companion to `config_rewrite.md`. This is the output of step 1 of the rewrite plan: a field-by-field, section-by-section reading of the current C++ implementation (`include/xrpl/basics/BasicConfig.h`, `src/libxrpl/basics/BasicConfig.cpp`, `src/xrpld/core/Config.h`, `src/xrpld/core/ConfigSections.h`, `src/xrpld/core/detail/Config.cpp`, plus per-section parsers spread across the codebase), the example config (`cfg/xrpld-example.cfg`), and a sweep of call sites.

Convention: file paths are repo-relative; line numbers are at the time of writing and may drift.

## 0. Summary of decisions

A condensed view of the decisions reached in §6 and §7 so step 2 (design) doesn't have to re-derive them. Each entry links back to the question that originated it.

### Overall policy

- **INI is lenient / compat-first by default.** Replicates existing `BasicConfig` behavior verbatim: silent-ignore of unknown keys and sections, per-field grammar rules unchanged, silent clamps preserved where they exist today. (§7 #10, §7 #11)
- **TOML is strict by default.** Unknown keys, unknown sections, out-of-range values, and trailing-junk in custom grammars are parse errors. (§7 #11, §7 #12)
- **Both formats produce the same typed `ParsedConfig`.** Schema is identical; only the surface grammar and strictness differ.

### Grammar

| Aspect | INI | TOML | Source |
|---|---|---|---|
| Booleans | `0\|1\|true\|false` case-insensitive | spec (`true\|false`) | §7 #1 |
| Numbers | decimal only, optional leading `+` | spec (`0x`, `0o`, `0b`, `_` separators OK) | §7 #2 |
| Durations | per-field grammar (existing) | per-field grammar; `amendment_majority_time` rejects trailing junk | §7 #3 + re-check |
| Case-sensitivity | case-sensitive (matches existing impl) | case-sensitive (TOML spec) | §7 #4 |
| Path resolution | piecemeal (3 fields auto-resolve) | uniform (all relative paths resolve against config dir) | §7 #5 + re-check |
| Out-of-range numerics | silent clamp where existing impl clamps | error | §7 #10 |
| Empty `key=` | reclassified as bare-value line (existing bug preserved) | rejected at TOML spec level | §6.11 |
| `\#` escape | kept (existing) | n/a (TOML uses its own string escapes) | §6.10 |
| Trailing comments (`# …`) | kept (existing) | n/a (TOML uses its own comment syntax) | §1.4 (no change needed) |

### Structural

- **Two types and a facade.** `ParsedConfig` (immutable, file-only) + `CliOverrides`/`ConfigOverrides` (mutable, post-parse) → `RuntimeConfig` (read facade: per-field accessor returns `cli.X.unwrap_or(parsed.X)`). (§7 #7, §7 #13)
- **INI parse strategy.** Two-stage: raw section bag → typed sub-structs via per-section adapters. (§6.1)
- **`[port_*]` layout.** INI flat (`[server]` + `[port_<name>]`). TOML table-of-tables (`server.ports = [...]` + `[port.<name>]`). (§7 #6)
- **Stray `[port.<name>]`** (not in `server.ports`): error in TOML, ignored in INI. (§6.2)
- **`[crawl]` dual shape.** INI accepts bare bool or kv map; TOML kv map only. (§6.1)
- **Other section groups (`[node_db]`, `[validators]`, `[validator_keys]`, `[validator_list_*]`, etc.).** Flat top-level tables in both INI and TOML. No nested restructuring. (§6.12)
- **`legacy()` accessor.** Retired. Every single-line section becomes a typed field on `ParsedConfig`. (§6.17)
- **Lifted fields (`FAST_LOAD`, `USE_TX_TABLES_`).** Drop the lift; consumers migrate to typed sub-structs (`node_db.fast_load`, `ledger_tx_tables.use_tx_tables`). (§6.13)
- **`[ips]` / `[ips_fixed]`.** Modeled as `Vec<HostPort>` with parsed values, not `Vec<String>` + post-pass regex. (§6.16)

### Scope

- **`kSIZED_ITEMS` and `NodeSize` auto-detection move to Rust.** The const table is exposed; `getValueFor` becomes an FFI call returning a primitive `int`. Auto-detection (RAM + CPU probe) runs in Rust via `sysinfo` / `available_parallelism` equivalents. (§7 #8)
- **`validators.txt` is spliced into the main config**, matching existing behavior. Strict-mode overlap (same section in both files) is an error in TOML, silent append in INI. (§7 #9)
- **Side effects:** Rust does anything that doesn't drag C++ types over the FFI — config file discovery via env vars, `mkdir -p` of the data dir, path absolutization, `validators.txt` ingest, `stderr` echo, cross-field validation, `NodeSize` detection. C++ keeps `HTTPClient::initializeSSLContext` and any other side effect that needs C++ class types. (§7 #14)
- **`FORCE_MULTI_THREAD`** lives on `CliOverrides`. It is a real runtime field (read by `Application.cpp:329`) that no config file sets; tests populate it directly. (§7 #15)

### Out-of-scope (deferred to step 2)

The shape of `getValueFor` on the Rust side — flat table lookup vs. per-`SizedItem` typed method — is a step-2 design choice (§7 #8). Likewise the naming of `CliOverrides`/`ConfigOverrides` and exact FFI binding choices (cxx-rs vs. C ABI) are step-2 calls.

## 1. The shape of the existing types

### 1.1 `BasicConfig` and `Section`

`BasicConfig` (`include/xrpl/basics/BasicConfig.h:202`) is a flat `unordered_map<string, Section>`. Sections are created lazily on access; `section(name)` on a const `BasicConfig` returns a static empty `Section const&` if missing (`src/libxrpl/basics/BasicConfig.cpp:127`).

`Section` (`include/xrpl/basics/BasicConfig.h:23`) carries **three views of the same section body**:

- `lines_` — every non-blank line, in input order, after comment stripping.
- `values_` — the subset of `lines_` that did **not** match the `key=value` regex.
- `lookup_` — `unordered_map<string,string>` of the key=value pairs (last write wins).

This shape is significant: many sections (`[ips]`, `[validators]`, `[features]`, `[cluster_nodes]`, `[rpc_startup]`, `[amendments]`, …) carry payload through `values_`/`lines_`, not `lookup_`. A few (`[debug_logfile]`, `[database_path]`, `[validators_file]`, `[network_id]`, etc.) carry a *single* bare line, fetched via the `legacy(...)` accessor. The Rust schema must model all three usage modes.

### 1.2 Parse pipeline

1. `parseIniFile` (`src/xrpld/core/detail/Config.cpp:164`) normalizes line endings (`\r\n` and `\r` → `\n`), splits on `\n`, trims, and emits an `IniFileSections` (a `map<section_name, vector<string>>`). Blank lines and lines starting with `#` are dropped here.
2. `BasicConfig::build` (`src/libxrpl/basics/BasicConfig.cpp:165`) iterates that map and calls `Section::append` once per section.
3. `Section::append` (`src/libxrpl/basics/BasicConfig.cpp:28`) walks each line, strips trailing `#…` comments (with `\#` escape), and tries the key/value regex; if it matches, the pair goes to `lookup_` and `lines_`; otherwise the raw line goes to `values_` and `lines_`.

Key consequence: **only `lookup_` is overwrite-on-duplicate; `lines_` and `values_` accumulate in order.** If the same section appears twice in a file, the lines concatenate.

### 1.3 Lexical-cast paths (a real footgun)

There are **three distinct boolean-parsing paths** in the existing codebase that the Rust replacement must reconcile:

| Path | Where | Accepts |
|---|---|---|
| `beast::lexicalCastThrow<bool>(str)` | most direct Config.cpp calls | `"0"`, `"1"`, `"true"`, `"false"` (case-insensitive) — see `include/xrpl/beast/core/LexicalCast.h:72` |
| `boost::lexical_cast<bool>(str)` via `Section::get<T>` / `valueOr<T>` / free `set<T>(…)` | most subsection key parsers (TxQ, port, sqlite, overlay…) | only `"0"` and `"1"` — Boost is strict |
| `getIfExists<bool>` template specialization | `include/xrpl/basics/BasicConfig.h:367` — parses as `int`, then `bool(int)` | any integer (non-zero → true) |

Integers go through `beast::lexicalCastThrow<T>` → `std::from_chars(…, 10)` (`include/xrpl/beast/core/LexicalCast.h:67`): **decimal only, optional leading `+`, no `0x`, no leading minus for unsigned, hard fail on overflow**. `boost::lexical_cast<int>` (via `Section::get`) is more permissive (accepts whitespace, leading zeros, etc.). This is another behavior split to flatten.

### 1.4 Comment, key, and value grammar

- Comment: `#…` strips the rest of the line; `\#` is an escape that leaves the `#` literal in place (`src/libxrpl/basics/BasicConfig.cpp:46-76`). Whole-line `#` comments are dropped earlier in `parseIniFile`. Section recognizes whether trailing comments were stripped and exposes `hadTrailingComments()`.
- Key regex (`src/libxrpl/basics/BasicConfig.cpp:31`): `[a-zA-Z][_a-zA-Z0-9]*` — must start with a letter, ASCII only. Identifiers are documented as case-insensitive in the example config, but the implementation uses `unordered_map<std::string,…>` keyed by the **exact** parsed key, so duplicates differing only in case are treated as distinct entries. The Rust rewrite should pick one rule explicitly.
- Value: `.*\S+` after `=` and whitespace; trailing whitespace is trimmed; an entirely empty value fails the regex and the line is reclassified as a non-kv `value`.
- Section header: `[name]` exact; no validation on the name body — spaces, dashes, dots all pass. A line like `[foo` (no closing bracket) is treated as a value of the *current* section.

### 1.5 Side effects baked into Section parsing

A few places in `Config::loadFromString` rewrite section content **after** parsing:

- `[ips]` and `[ips_fixed]`: a regex (`":([0-9]+)$"`) rewrites `host:port` → `host port`, but only when the line has exactly one colon (to avoid clobbering IPv6) — `src/xrpld/core/detail/Config.cpp:488-507`.
- `[validators_file]`: a separately-parsed INI is *merged* into the running `BasicConfig` — its `[validators]`, `[validator_keys]`, `[validator_list_sites]`, `[validator_list_keys]`, `[validator_list_threshold]` are appended into the main config's sections (`src/xrpld/core/detail/Config.cpp:1011-1046`).
- `[validators]` and `[validator_keys]` are then consolidated: the latter's lines are appended into the former (`:1094`).

The Rust rewrite must reproduce these rewrites or restructure the consuming code.

## 2. Field inventory of `Config`

Categorized by where the value comes from. Each entry: `FIELD : type = default — source — consumed by (sample)`.

### 2.1 Fields populated from the **config file** (during `load()` / `loadFromString`)

#### Bare top-level controls

- `IPS : vector<string>` — `[ips]` lines (post host:port rewrite). Consumer: `PeerfinderConfig.cpp`.
- `IPS_FIXED : vector<string>` — `[ips_fixed]` lines (post rewrite). Consumer: `PeerfinderConfig.cpp`.
- `NETWORK_ID : uint32_t = 0` — `[network_id]` single line; `"main"=0`, `"testnet"=1`, `"devnet"=2`, else integer. Consumer: `OverlayImpl.cpp`, `Handshake.cpp`.
- `NETWORK_QUORUM : size_t = 1` — `[network_quorum]` single line; validated against `PEERS_MAX` (≤ 21 if `PEERS_MAX==0`).
- `PEER_PRIVATE : bool = false` — `[peer_private]`, parsed via `beast::lexicalCastThrow<bool>`.
- `PEERS_MAX : size_t = 0`, `PEERS_IN_MAX : size_t = 0`, `PEERS_OUT_MAX : size_t = 0` — `[peers_max]`, `[peers_in_max]`, `[peers_out_max]`. Rules: if `peers_max` is set, it wins; otherwise `peers_in_max` (must be ≤ 1000) and `peers_out_max` (must be 10–1000) must both be set or both unset.
- `RELAY_UNTRUSTED_VALIDATIONS : int = 1`, `RELAY_UNTRUSTED_PROPOSALS : int = 0` — `[relay_validations]` / `[relay_proposals]` single line, values `"all"=1`, `"trusted"=0`, `"drop_untrusted"=-1`.
- `NODE_SIZE : size_t = 0` — `[node_size]` single line; values `tiny|small|medium|large|huge` map to 0..4, else integer clamped to ≤ 4.
- `signingEnabled_ : bool = false` — `[signing_support]`. Accessor `canSign()`.
- `ELB_SUPPORT : bool = false` — `[elb_support]`. Used to gate the load-balancer health endpoint.
- `SSL_VERIFY : bool = true` — `[ssl_verify]`.
- `SSL_VERIFY_FILE : string` — `[ssl_verify_file]`, single line (path).
- `SSL_VERIFY_DIR : string` — `[ssl_verify_dir]`, single line (path).
- `FEES : FeeSetup` — `[voting]` map keys `reference_fee`, `account_reserve`, `owner_reserve`. `[fee_default]` single line overrides `reference_fee`. Parsed via `setupFeeVote` (`src/xrpld/core/detail/Config.cpp:1182`).
- `LEDGER_HISTORY : uint32 = 256` — `[ledger_history]` single line; `"full"=UINT32_MAX`, `"none"=0`, else integer. Forced to 0 in standalone mode.
- `FETCH_DEPTH : uint32 = 1_000_000_000` — `[fetch_depth]` single line; `"none"=0`, `"full"=UINT32_MAX`, else integer; floored at 10.
- `PATH_SEARCH_OLD : int = 2`, `PATH_SEARCH : int = 2`, `PATH_SEARCH_FAST : int = 2`, `PATH_SEARCH_MAX : int = 3` — `[path_search*]`, single int. `PATH_SEARCH_MAX` is force-set to 0 if `[validation_seed]` or `[validator_token]` is present (validator default disables pathfinding).
- `MAX_TRANSACTIONS : int = 250` — `[max_transactions]` single line, clamped to `[100, 1000]`.
- `AMENDMENT_MAJORITY_TIME : seconds` (default `kDEFAULT_AMENDMENT_MAJORITY_TIME`) — `[amendment_majority_time]` single line with a custom grammar: regex `^\s*(\d+)\s*(minutes|hours|days|weeks)\s*(\s+.*)?$`. Floor: 15 minutes.
- `WORKERS : int = 0`, `IO_WORKERS : int = 0`, `PREFETCH_WORKERS : int = 0` — `[workers]` / `[io_workers]` / `[prefetch_workers]`, each clamped to `[1, 1024]`.
- `SWEEP_INTERVAL : optional<int>` — `[sweep_interval]` single line, clamped to `[10, 600]`.
- `COMPRESSION : bool = false` — `[compression]`.
- `LEDGER_REPLAY : bool = false` — `[ledger_replay]`.
- `BETA_RPC_API : bool = false` — `[beta_rpc_api]`.
- `SERVER_DOMAIN : string` — `[server_domain]` single line, validated via `isProperlyFormedTomlDomain`.
- `MAX_UNKNOWN_TIME : seconds = 600`, `MAX_DIVERGED_TIME : seconds = 300` — `[overlay]` keys `max_unknown_time` (300..1800) and `max_diverged_time` (60..900).
- `features : unordered_set<uint256>` — `[features]` parsed from `values_`: each line is a feature *name*; throws on unknown name via `getRegisteredFeature`.
- `VALIDATOR_LIST_THRESHOLD : optional<size_t>` — `[validator_list_threshold]` from `values_`: single integer; 0 means "compute"; must not exceed number of `validator_list_keys` entries.
- `DEBUG_LOGFILE_ : path` — `[debug_logfile]` single line; resolved relative to `CONFIG_DIR` lazily by `getDebugLogFile()` (`src/xrpld/core/detail/Config.cpp:1138`).
- Reduce-relay subblock from `[reduce_relay]` map:
  - `VP_REDUCE_RELAY_BASE_SQUELCH_ENABLE : bool = false` — `vp_base_squelch_enable` (or legacy `vp_enable`; setting both is an error).
  - `VP_REDUCE_RELAY_SQUELCH_MAX_SELECTED_PEERS : size_t = 5` — `vp_base_squelch_max_selected_peers`, must be ≥ 3.
  - `TX_REDUCE_RELAY_ENABLE : bool = false` — `tx_enable`.
  - `TX_REDUCE_RELAY_METRICS : bool = false` — `tx_metrics`.
  - `TX_REDUCE_RELAY_MIN_PEERS : size_t = 20` — `tx_min_peers`, must be ≥ 10.
  - `TX_RELAY_PERCENTAGE : size_t = 25` — `tx_relay_percentage`, must be in `[10,100]`.

#### Fields read after `load()` from sections never lifted onto `Config`

These sections are kept inside `BasicConfig` and read by other modules. The Rust rewrite owns *all* of them — but they don't surface on a `Config::FOO` field. Listed in §3.

#### Stored on `Config` but updated from a *different* section than the name suggests

- `USE_TX_TABLES_ : bool = true` — read in `Config::setup` from `[ledger_tx_tables]` key `use_tx_tables` (not from a top-level `[use_tx_tables]`).
- `FAST_LOAD : bool = false` — read in `Config::setup` from `[node_db]` key `fast_load`. (The other `[node_db]` keys are consumed by `SHAMapStoreImp`/`NodeStore`, not by `Config`.)

### 2.2 Fields populated from the **command line** (not the file)

These are written from `src/xrpld/app/main/Main.cpp` after `Config::setup` runs. The Rust replacement should expose mutators (or a builder) for these:

| Field | CLI source (Main.cpp ~line) |
|---|---|
| `START_UP : StartUpType = Normal` | `--start`, `--ledger`, `--ledgerfile`, `--load`, `--replay`, `--net` |
| `START_LEDGER : string` | `--ledger`, `--ledgerfile` |
| `START_VALID : bool = false` | `--valid` |
| `TRAP_TX_HASH : optional<uint256>` | `--trap_tx_hash` |
| `doImport : bool = false` | `--import` |
| `FORCED_LEDGER_RANGE_PRESENT : optional<(u32,u32)>` | `--force_ledger_present_range` |
| `VALIDATION_QUORUM : optional<size_t>` | `--quorum` |
| `rpc_ip : optional<IP::Endpoint>` | `--rpc_ip` (RPC client mode) |

### 2.3 Fields set from `setupControl` (constructor args)

- `QUIET_ : bool = false` — from `--quiet` (or `--silent` which implies it).
- `SILENT_ : bool = false` — from `--silent`.
- `RUN_STANDALONE_ : bool = false` — from `--standalone`.

### 2.4 Fields populated by auto-detection / hardware probing

- `ramSize_ : uint64_t` (GiB) — read from OS at construction (Win: `GlobalMemoryStatusEx`; Linux: `sysinfo`; macOS: `sysctl HW_MEMSIZE`). Read-only after that.
- `NODE_SIZE` — in `setupControl`, if `[node_size]` was not given: walk the `RamSizeGb` row of `kSIZED_ITEMS` to pick the first bucket that fits available RAM; then cap by `hardware_concurrency() / 2`. Forced to 0 (`"tiny"`) for standalone mode.

### 2.5 Fields used by tests only

- `FORCE_MULTI_THREAD : bool = false` — never read from any config file; written only from C++ tests (`Coroutine_test.cpp:61,88`, `Transaction_ordering_test.cpp:67,105`). But *read* by production code in `Application.cpp:329` (job-queue sizing — in standalone mode, the queue uses 1 worker unless this flag is set). So it's a real runtime knob that lives outside the file, populated only via the override path.

### 2.6 Computed accessors

- `Config::getValueFor(SizedItem item, optional<size_t> node)` indexes a 13×5 constexpr table `kSIZED_ITEMS` (`src/xrpld/core/detail/Config.cpp:114-137`) keyed by `(item, node_size)`. The table is the central place where many runtime defaults live (sweep interval, tree cache size, ledger cache sizes, account-id cache size, etc.) The Rust port must surface this table; ~16 call sites rely on it.
- `Config::getDebugLogFile()` lazily resolves `DEBUG_LOGFILE_` relative to `CONFIG_DIR` and creates the parent directory on demand.

## 3. Sections kept inside `BasicConfig` and parsed elsewhere

All section consumers must keep working. The Rust schema must either expose typed sub-structs *or* keep a raw "section bag" escape hatch. Below, each section's shape, parser, and key catalog.

### 3.1 Server / ports (the only *dynamic* schema)

- `[server]` — **mixed shape**: `values_` holds the list of port subsection names; `lookup_` holds shared defaults applied to all ports. Parser: `src/xrpld/rpc/detail/ServerHandler.cpp:1139` (`parsePorts`).
- `[<port_name>]` — one subsection per name in `[server].values()`. Parser: `src/libxrpl/server/Port.cpp:194` (`parsePort`). Keys (all optional unless noted):

  `ip`, `port` (uint16 > 0 — Config.cpp explicitly rejects port=0 in `checkZeroPorts`), `protocol` (comma list of `http|https|ws|wss|peer|grpc` — peer protocol only allowed on a single port), `limit` (`"unlimited"` or int, default unlimited), `send_queue_limit` (uint16 > 0, default 100), `admin` (CIDR list), `secure_gateway` (CIDR list), `user`, `password`, `admin_user`, `admin_password`, `ssl_key`, `ssl_cert`, `ssl_chain`, `ssl_ciphers`, `permessage_deflate` (bool, default true), `client_max_window_bits` (9–15, default 15), `server_max_window_bits` (9–15, default 15), `client_no_context_takeover` (bool, default false), `server_no_context_takeover` (bool, default false), `compress_level` (0–9, default 8), `memory_level` (1–9, default 4), `ssl_cert_chain`, `ssl_client_ca` (gRPC mTLS).

  This is the **hardest** section to model in the Rust rewrite: the section *names* are user-chosen and only enumerated via `[server].values()`. Two reasonable options:
  - Represent as `BTreeMap<String, PortConfig>` plus a `Vec<String>` listing.
  - In TOML, naturally expressed as `[port.<name>]` table-of-tables; in INI we keep the existing convention.

### 3.2 Database

- `[node_db]` (required) and `[import_db]` (optional, used only with `--import`) — same schema. Parser: `src/xrpld/app/misc/SHAMapStoreImp.cpp:104` and the `NodeStore::Manager` factory. Keys: `type` (`NuDB` | `RocksDB`, required), `path` (required), `fast_load` (bool — also lifted onto `Config::FAST_LOAD`), `earliest_seq` (uint32 ≥ 1, default 32570), `online_delete` (uint32, ≥ 256 when set), `advisory_delete` (bool, default false), `delete_batch` (uint32, default 100), `back_off_milliseconds` (uint32, default 100), `age_threshold_seconds` (uint32, default 60), `recovery_wait_seconds` (uint32, default 5), `nudb_block_size` (uint32 power-of-2 ∈ [4096, 32768], default 4096; NuDB only), plus RocksDB-only tunables (`cache_mb`, `filter_bits`, …) auto-derived from `NODE_SIZE` when unset.
- `[database_path]` — single bare line. Read via `BasicConfig::legacy("database_path")`. `Config::setup` resolves it to an absolute path and writes the resolved value back into the section (this is the only field that the existing implementation *mutates* in place). Default: `<config_dir>/db` unless `RUN_STANDALONE_`.
- `[sqlite]` — Parser: `setupDatabaseCon` in `src/xrpld/core/detail/Config.cpp:1201`. Keys: `safety_level` (`high|low`), `journal_mode` (`delete|truncate|persist|memory|wal|off`), `synchronous` (`off|normal|full|extra`), `temp_store` (`default|file|memory`), `page_size` (int, power-of-2 ∈ [512, 65536], default 4096), `journal_size_limit` (int, default 1582080). Mutual exclusion: `safety_level` cannot coexist with `journal_mode`, `synchronous`, or `temp_store`. Logs a warning if low-safety + `LEDGER_HISTORY > kSQLITE_TUNING_CUTOFF`.
- `[ledger_tx_tables]` — Parser: `Config::setup` `:417`. Currently only `use_tx_tables` (bool) is read.
- `[relational_db]` — referenced in `ConfigSections.h`; backend selection (sqlite for now).

### 3.3 Overlay / peer protocol

- `[overlay]` map. Parser: partly in `Config::loadFromString` (`max_unknown_time`, `max_diverged_time`) and partly in `OverlayImpl::setup_Overlay` (`public_ip`, `ip_limit`). Keys: `public_ip` (IPv4 dotted), `ip_limit` (int, auto if unset), `max_unknown_time` (uint32 300..1800, default 600), `max_diverged_time` (uint32 60..900, default 300).
- `[crawl]` — **dual shape**: either a single legacy boolean line, or a map of `overlay|server|counts|unl` (booleans). Parser: `OverlayImpl::setup_Overlay`.
- `[vl]` map. Keys: `enabled` (bool). Parser: `OverlayImpl::setup_Overlay`.
- `[sntp_servers]` — bare-line list. Consumer: `SNTPClock::set_servers`.
- `[cluster_nodes]` — bare-line list of `<node_public_key> [name]`. Consumer: `Application::setup`.

### 3.4 Consensus / amendments / fees

- `[voting]` — already lifted onto `Config::FEES` (see §2).
- `[fee_default]` — single bare line; same field.
- `[amendments]` — bare-line list of `<64-hex-amendment-id> <name>`. Parser: `src/xrpld/app/misc/detail/AmendmentTable.cpp` (`parseSection`). Side effect: writes initial Up votes to wallet.db.
- `[veto_amendments]` — same shape; writes Down votes.

### 3.5 Validator identity / lists

- `[validation_seed]` — single bare line (base58 family seed). Mutually exclusive with `[validator_token]` (enforced in `Config::loadFromString` `:668`).
- `[validator_token]` — multi-line base64 token (one record, possibly wrapped).
- `[validator_key_revocation]` — single bare line (base64 revocation blob).
- `[validators]`, `[validator_keys]` — bare-line lists; consolidated in `Config::loadFromString` (latter appended into former).
- `[validator_list_sites]`, `[validator_list_keys]`, `[validator_list_threshold]` — bare-line lists (single value for threshold).
- `[validators_file]` — single bare line pointing to a separate INI; that file's `[validators] / [validator_keys] / [validator_list_*]` sections are merged into the main config (see §1.5). Resolved relative to `CONFIG_DIR`. If specified, must exist and be a regular file or symlink; if not specified, `<CONFIG_DIR>/validators.txt` is tried silently.

### 3.6 RPC / API

- `[rpc_startup]` — bare-line list of JSON command objects, run at startup.
- `[port_grpc]` — handled like other port subsections (gRPC has its own factory in `GRPCServer`).
- `[beta_rpc_api]` — lifted onto `Config::BETA_RPC_API`.

### 3.7 Transaction queue

- `[transaction_queue]` map. Parser: `src/xrpld/app/misc/detail/TxQ.cpp` `setup_TxQ`. Keys: `ledgers_in_queue` (uint, default 20), `minimum_queue_size` (uint, default 2000), `retry_sequence_percent` (uint, default 25), `minimum_escalation_multiplier` (uint, default 500 × `kBASE_LEVEL`), `minimum_txn_in_ledger` (uint, default 32), `minimum_txn_in_ledger_standalone` (uint, default 1000), `target_txn_in_ledger` (uint, default 256), `maximum_txn_in_ledger` (optional uint, must ≥ min), `normal_consensus_increase_percent` (uint clamped 0..1000, default 20), `slow_consensus_decrease_percent` (uint clamped 0..100, default 50), `maximum_txn_per_account` (uint, default 10), `minimum_last_ledger_buffer` (uint, default 2), `zero_basefee_transaction_feelevel` (uint, default 256000).

### 3.8 Observability

- `[debug_logfile]` — lifted onto `Config::DEBUG_LOGFILE_`.
- `[insight]` map. Keys: `server` (only `"statsd"` recognised), `address` (`ip:port`), `prefix` (string).
- `[perf]` map. Keys: `perf_log` (path, relative to config dir; required to enable), `log_interval` (uint seconds, default 1).
- `[websocket_ping_frequency]` — single bare line (uint seconds).

### 3.9 Documented in the example but not currently parsed

The example config documents `[transaction_queue]` keys as EXPERIMENTAL; the example also mentions `[node_seed]` (clustering — currently parsed by clustering code), `[crawl]` and `[vl]` (handled), and a vacated section 5. No "orphan" documented sections were found — every section in `cfg/xrpld-example.cfg` has a real consumer.

## 4. Side effects baked into `Config::setup`/`load`

These must be preserved in the migration. Many are not really "config parsing" — they are *bootstrap* logic that happens to live inside the Config class:

1. **Config file discovery** (`Config::setup`): if `--conf` was given, use it verbatim. Otherwise, in order: `./xrpld.cfg`, `./rippled.cfg`, `$XDG_CONFIG_HOME/<systemName>/{xrpld,rippled}.cfg` (with fallback `$HOME/.config/<systemName>`), `/etc/opt/<systemName>/{xrpld,rippled}.cfg`. The last-tried path is kept even if it doesn't exist.
2. **Data directory resolution**: `dataDir = <config_dir>/db` by default; overridden by `[database_path]`; cleared if standalone; otherwise `create_directories(dataDir)` is called and the absolute path is written back via `legacy("database_path", …)`. So **the running `Config` is *mutated* by setup**, not just populated.
3. **`HTTPClient::initializeSSLContext`** is called from `Config::setup` (`:410`) using `SSL_VERIFY_*` fields. This is a global side effect: the SSL context is process-wide.
4. **`LEDGER_HISTORY = 0` in standalone mode** is forced *after* parsing.
5. **`PATH_SEARCH_MAX = 0` if `[validation_seed]` or `[validator_token]` is present** is forced before per-key path-search parsing.
6. **`USE_TX_TABLES_` and `FAST_LOAD`** are pulled from foreign sections at the tail of `Config::setup` (`:417`, `:420`).
7. **Reading `validators.txt`** and merging its sections into `BasicConfig` (§1.5).
8. **`checkZeroPorts`** runs at the end of `Config::load` and walks `[server].values()` to forbid `port=0` (`:425`).
9. **Echoing the file path to `stderr`** when not in quiet mode (`:457`).
10. **Environment variables** read directly: `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`. (Not generally extensible.)
11. **`Config::getDebugLogFile()` creates the log directory** on the fly.

## 5. Validators / range constraints in the existing code

Compiled in one place so the Rust validator pass has a starting list:

| Section / key | Constraint |
|---|---|
| `peers_in_max` | ≤ 1000 |
| `peers_out_max` | 10..1000 |
| `peers_in_max` + `peers_out_max` | both-or-neither |
| `sweep_interval` | 10..600 |
| `workers`, `io_workers`, `prefetch_workers` | 1..1024 |
| `max_transactions` | clamped to 100..1000 (silent clamp, not error) |
| `fetch_depth` | min 10 (silent floor) |
| `overlay.max_unknown_time` | 300..1800 |
| `overlay.max_diverged_time` | 60..900 |
| `reduce_relay.vp_base_squelch_max_selected_peers` | ≥ 3 |
| `reduce_relay.tx_min_peers` | ≥ 10 |
| `reduce_relay.tx_relay_percentage` | 10..100 |
| `reduce_relay.vp_enable` + `vp_base_squelch_enable` | not both |
| `amendment_majority_time` | duration grammar; ≥ 15 minutes |
| `validation_seed` + `validator_token` | not both |
| `validator_list_threshold` | ≤ # of validator_list_keys; 0 means "compute" |
| `validators_file` | must exist (regular file or symlink) if explicitly named |
| `network_quorum` | ≤ effective peers_max (defaulted to 21 if unset) |
| `sqlite.safety_level` | not combined with `journal_mode`/`synchronous`/`temp_store` |
| `sqlite.journal_mode` | `delete|truncate|persist|memory|wal|off` |
| `sqlite.synchronous` | `off|normal|full|extra` |
| `sqlite.temp_store` | `default|file|memory` |
| `sqlite.page_size` | power of 2 in 512..65536 |
| `node_db.online_delete` | ≥ `ledger_history` (cross-section) |
| `node_db.nudb_block_size` | power of 2 in 4096..32768 |
| `port_*.port` | > 0 (zero only allowed in unit tests, never in the file) |
| `port_*.send_queue_limit` | > 0 |
| port `protocol` | values from `{http,https,ws,wss,peer,grpc}`; `peer` on at most one port; ws/non-ws not mixed |
| `server_domain` | passes `isProperlyFormedTomlDomain` |

## 6. Edge cases and open questions

Sorted by how impactful they are to the Rust design.

### 6.1 Sections that have *both* `key=value` and bare-line content

`[server]` is the canonical one (bare port names + shared kv defaults). `[crawl]` is dual-shape (single bare bool *or* kv map). `[transaction_queue]` only uses kv but had legacy single-line usage in older configs. **Open question:** how to express this in `serde`? A union with custom `Deserialize` per section is feasible but ugly. The two-stage approach (raw section bag → typed) keeps custom deserialization small. **Tentative recommendation:** keep the two-stage parse for INI; in TOML, define a canonical schema (`server.ports = ["peer", "rpc_admin"]` plus `[port_*]` tables) and let serde handle it directly.

**Decision (INI strategy):** Two-stage parse. Stage 1 produces a raw section bag (`Vec<(name, Vec<line>)>`, close to today's `IniFileSections`). Stage 2 decodes each section into its typed sub-struct via per-section adapters. Dual-shape sections like `[server]` and `[crawl]` get small custom decoders rather than awkward `serde` `untagged` enums. Bare-line list sections (`[validators]`, `[features]`, `[ips]`, …) are decoded directly from their line vector. TOML uses the standard `serde` deserialization path.

**Decision (`[crawl]`):** Accept both shapes in INI — single bare bool line *or* kv map — matching existing behavior. TOML accepts the kv-map form only (the bare-bool form has no clean TOML equivalent).

### 6.2 `[port_*]` is a *dynamic* set of sections

Port names are user-chosen, only enumerated via `[server].values()`. **Open question:** how do unknown sections in strict mode interact with `[port_*]`? Proposal: validate against the union of (known top-level sections) ∪ (the names listed in `[server]`).

**Decision:** TOML strict — every `[port.<name>]` table must have a matching entry in `server.ports = [...]`; an orphan table is a parse error. INI lenient — matches existing behavior: `[port_foo]` without a matching name in `[server]` is silently ignored (today `ServerHandler` only iterates the names listed in `[server].values()`, so orphans never get read). This is the natural consequence of the §7 #11 INI=lenient / TOML=strict split.

### 6.3 Three boolean parsing paths today (§1.3)

The Rust rewrite collapses to one rule. **Tentative recommendation:** match the most permissive existing path (`beast::lexicalCastThrow<bool>`): accept `0`, `1`, `true`, `false`, case-insensitive. Reject everything else. This breaks any field that today happens to be set as `yes`/`no` and accidentally passed through `getIfExists<bool>` → int → bool with non-zero=true, but I could not find any user-facing examples.

**Decision:** Resolved by §7 #1 — INI accepts `0|1|true|false` case-insensitive; TOML uses spec.

### 6.4 Numeric grammar

Today's path is decimal-only via `std::from_chars`. Rust's `i64::from_str` matches that. **Open question:** do we want to accept human-friendly units (e.g. `64M`, `2GB`) in any place? Today nothing does. Recommend: no, keep strictly decimal. Cap and clamp behavior (e.g. `max_transactions` silently clamped) becomes an *explicit* validation rule with an error message rather than a silent clamp.

**Decision:** Resolved by §7 #2 (numeric grammar — decimal only in INI; TOML follows spec) and §7 #10 (silent clamp kept in INI; error in TOML).

### 6.5 Duration grammar

`[amendment_majority_time]` uses a custom regex `(\d+)\s*(minutes|hours|days|weeks)`. TOML has no native duration; INI tradition here is the same. Recommend: define a `Duration` type with one canonical grammar (`<int><space?><unit>`, units `minutes|hours|days|weeks|seconds`), and reuse it for `overlay.max_unknown_time` etc. (today those are bare integer seconds — slight expansion of the grammar, but backwards-compatible if integers without a unit are still seconds).

**Decision:** Resolved by §7 #3 — per-field grammar, matching existing. No unified `Duration` type in INI.

### 6.6 Path-relative-to-config-file semantics

Currently only `debug_logfile`, `validators_file`, and `database_path` are auto-resolved. `ssl_*` paths, `node_db.path`, `perf.perf_log`, `port_*.ssl_*` are *not* auto-resolved (user is expected to provide an absolute path). Recommend: pick one rule and apply uniformly, ideally "resolve relative to the config file". Make this explicit in the schema (a `RelPath` vs `Path` type, or a single `Path` type that always resolves).

**Decision:** Resolved by §7 #5 — piecemeal, matching existing. Only `debug_logfile`, `validators_file`, `database_path` auto-resolve.

### 6.7 `validators.txt` is a nested INI

The current implementation parses a separate file and **splices** its sections into the running config. Two equivalent strategies in Rust:
- Keep the splice model: read both files, merge sections (forbid overlap between top-level and validators file).
- Make `validators_file` a typed sub-document of its own, exposed under a dedicated submodule of `Config`.

The second is cleaner but breaks existing consumers that look up `config.section("validators")` etc. Pick this when migrating consumers.

**Decision:** Resolved by §7 #9 — splice model, matching existing.

### 6.8 Mutation after parsing

The existing `Config` is mutated after `setup()` in two distinct ways:
- `BasicConfig::legacy("database_path", absolute_path)` is written back from `Config::setup`.
- Several `Config::FOO` fields are written from `Main.cpp` based on command-line flags.

The first is internal bookkeeping (rewriting a parsed value to its absolute form) and is easy to absorb into the typed schema (just store the absolute path). The second is part of the public surface: the Rust API needs a builder/mutator path for CLI overrides. **Tentative recommendation:** parse the config file into an immutable `ParsedConfig`, then layer CLI overrides on top in a `RuntimeConfig` builder (CLI fields are a separate struct, joined at the end).

**Decision:** Resolved by §7 #7 (ParsedConfig + CliOverrides → RuntimeConfig facade) and §7 #13 (resolved paths live on RuntimeConfig).

### 6.9 Section name case-sensitivity

The example config (lines 64–66) documents identifiers as "not case sensitive". The implementation does not lowercase keys before lookup. Behavior diverges silently today: `[OVERLAY]` and `[overlay]` would be two distinct entries. **Open question:** match the documented "case-insensitive" rule (lowercase before lookup), or match the implementation (case-sensitive)? Recommend matching the documentation, since real configs almost certainly use lowercase already and the strict-mode rejection of stray-cased names would catch typos.

**Decision:** Resolved by §7 #4 — case-sensitive, matching the existing implementation. Example-cfg documentation will be corrected to match.

### 6.10 `\#` escape

The current escape rule (`\#` keeps `#` literal, *and* the `\` is removed) is unusual; YAML/TOML don't do this. It looks unused in the example config but is preserved in `Section::append`. Recommend: keep it for INI compatibility; ignore for TOML (TOML strings have their own escape rules).

**Decision:** Keep the `\#` escape in INI, matching existing `Section::append` behavior verbatim. TOML uses its own string escapes per the TOML spec; the rule is irrelevant there.

### 6.11 Empty values vs missing keys

In the current code, `key=` (empty value) fails the regex and falls through to `values_` — i.e. the key disappears and the line becomes a bare value. This is almost certainly a bug. Recommend: in strict mode, reject `key=` with an explicit error.

**Decision:** Reproduce existing INI behavior verbatim — `key=` fails the kv regex and becomes a bare-value line. The "key" is effectively lost (consumers asking for it via the kv lookup will not find it). Lossy but compat-first. TOML naturally rejects `key =` at the spec level (it's a syntax error). The behavior may be tightened in a future INI-strictness pass.

### 6.12 Section header inside a section body

`[name]` lines inside a section reset the parser's notion of "current section" — there is no nesting; you can't have a section inside a section. TOML supports tables-of-tables, and the rewrite of `[port_*]` may want to use that shape in TOML mode. INI mode keeps the flat layout. **Open question:** define the canonical TOML layout per section group, even if INI has to splice differently.

**Decision:** TOML layout mirrors INI section names 1:1 except for `[port_*]` (resolved in §7 #6: `[port.<name>]`). All other section groups stay flat in TOML — `[node_db]`, `[import_db]`, `[validators]`, `[validator_keys]`, `[validator_list_sites]`, `[validator_list_keys]`, `[validator_list_threshold]`, `[overlay]`, `[sqlite]`, etc. remain top-level tables. Rationale: keeps `--convert-config` simple (mostly a section-rename pass) and lets operators read INI and TOML side-by-side without surprise.

### 6.13 Sections silently lifted from `[node_db]` and `[ledger_tx_tables]` onto `Config`

Today the Config class reaches into the `BasicConfig` map to copy `fast_load` and `use_tx_tables` onto `Config` fields. This couples Config to the node-store and ledger-table schemas. Recommend: expose typed sub-structs (`config.node_db.fast_load`, `config.ledger_tx_tables.use_tx_tables`) and update consumers.

**Decision:** Drop the lift. `ParsedConfig.node_db.fast_load` and `ParsedConfig.ledger_tx_tables.use_tx_tables` are the canonical locations. Existing C++ consumers (`config->FAST_LOAD`, `config->useTxTables()`) migrate to the new paths during the step 4 C++ migration.

### 6.14 Default node size depends on RAM and CPU

`Config::setupControl` picks a `NODE_SIZE` based on installed RAM and `hardware_concurrency()`. Useful default — but it means **the parsed config is not deterministic w.r.t. file contents alone**. The Rust schema should preserve this auto-detection but isolate it behind a `Config::detect()` helper that runs *after* file parsing, so unit tests can substitute a fixed value.

**Decision:** Resolved by §7 #8 — auto-detection moves to Rust along with `kSIZED_ITEMS`. Isolated behind a deterministic step so tests can inject a fixed `NodeSize`.

### 6.15 `kSIZED_ITEMS` table

`getValueFor(SizedItem)` is the largest single mechanism for "what the actual runtime default is" — a 13-item × 5-size table. ~16 call sites use it. The Rust port must surface this table; recommend: expose as a `const` Rust table with a typed `node_size: NodeSize` enum.

**Decision:** Resolved by §7 #8 — table moves to Rust, exposed as a `const` table with a typed `NodeSize` enum. `getValueFor` becomes an FFI call returning a primitive `int`.

### 6.16 Order-sensitivity of `[ips]` post-processing

The colon-rewrite happens *after* `BasicConfig::build`, mutating `Config::IPS` / `Config::IPS_FIXED` in place. If we model `[ips]` as `Vec<HostPort>` directly, the rewrite is just the parser for that type — cleaner.

**Decision:** Model as `Vec<HostPort>`. `HostPort = { host: String, port: Option<u16> }`. The parser accepts both `host port` and `host:port` (the colon form requires exactly one `:` to avoid IPv6 collisions, matching the existing rewrite regex). Bare `host` parses to `port: None`. IPv6 addresses with port use the space-separated form `fe80::1 51235`. Consumers (`PeerfinderConfig`) get typed values; the existing post-pass disappears.

### 6.17 The `legacy()` accessor

`BasicConfig::legacy("foo")` is used as "the single-line bare value of section foo". This is purely an artifact of representing single-value sections through the same `Section` machinery as everything else. In Rust we should just expose each such field as a typed field on the appropriate sub-struct, and retire the term.

**Decision:** Retired. Every single-line section becomes a typed field on `ParsedConfig` (e.g. `database_path: PathBuf`, `network_id: u32`, `debug_logfile: Option<PathBuf>`, `validators_file: Option<PathBuf>`, `node_size: Option<NodeSize>`, …). C++ consumers that today call `config.legacy("network_id")` etc. migrate to typed accessors during step 4. No `legacy(name)` shim is provided — the migration is mechanical and the shim would just delay it.

---

## 7. Open questions for review before step 2

The items below need a decision before the Rust scaffolding is laid down. Most have a tentative recommendation in §6.

1. **Boolean grammar.** Lock in: accept `0|1|true|false` case-insensitive, reject everything else. Anything missing?

   **Decision:** INI accepts `0|1|true|false` case-insensitive; reject everything else (no `yes`/`no`/`on`/`off`). TOML uses its spec-defined `true|false` only (handled by serde). Scope of this rule is the INI deserializer; TOML inherits from the format spec.
2. **Numeric grammar.** Decimal only, optional leading `+`, no `0x`, no human-friendly units. Confirm?

   **Decision:** INI numbers are decimal only, optional leading `+`, no `0x`/`0b`/`0o`, no human-friendly units (`64M`, `2GB`, `10s`). Hard fail on overflow. TOML numbers follow the TOML spec (which permits `0x`, `0o`, `0b`, and `_` separators). Scope: INI deserializer only.
3. **Duration grammar.** One grammar (`<int>[unit]`, units `seconds|minutes|hours|days|weeks`, default unit `seconds`) used everywhere. Confirm?

   **Decision:** Per-field grammar, matching existing behavior. `amendment_majority_time` keeps its `<int> minutes|hours|days|weeks` regex with ≥ 15 minutes floor. All other duration fields (`overlay.max_unknown_time`, `overlay.max_diverged_time`, `sweep_interval`, `[perf].log_interval`, `[node_db].age_threshold_seconds`, `[node_db].recovery_wait_seconds`, `[node_db].back_off_milliseconds`, `[websocket_ping_frequency]`) stay bare integers in their named unit. No unified `Duration` type in INI.

   **Format-asymmetric refinement (per #10/#11):** TOML rejects trailing junk in `amendment_majority_time` (grammar tightens to `^\s*(\d+)\s*(minutes|hours|days|weeks)\s*$`). INI keeps the existing loose regex that quietly accepts trailing content. Other duration fields are bare ints in both formats; no grammar difference there.
4. **Case-sensitivity** of section names and keys: lowercase before lookup (matching the example-cfg documentation) or keep case-sensitive (matching today's implementation)?

   **Decision:** Case-sensitive, matching the existing C++ implementation. Section names and keys are looked up by their exact parsed string. Strict mode will reject mis-cased names as "unknown section/key". The example-cfg documentation will be corrected to match. *Per-section enum values* (e.g. `node_size = TINY` vs `tiny`, `safety_level = HIGH` vs `high`, `relay_validations = ALL` vs `all`) remain case-insensitive where the existing per-field parsers already use `boost::iequals` — that is field-level behavior, not parser-level.
5. **Path resolution.** Apply "resolve relative to config file unless absolute" uniformly to all path-typed fields, or keep today's piecemeal behavior?

   **Decision:** Piecemeal, matching existing behavior. Only these three paths are auto-resolved relative to the config directory: `debug_logfile`, `validators_file`, `database_path`. All other path-typed fields (`ssl_verify_file`, `ssl_verify_dir`, `node_db.path`, `[perf].perf_log`, `port_*.ssl_key`/`ssl_cert`/`ssl_chain`/`ssl_cert_chain`/`ssl_client_ca`) are taken verbatim. The schema should mark which paths are auto-resolved with a distinct type or annotation so this rule is visible in code.

   **Format-asymmetric refinement (per #11):** TOML applies the uniform rule — every relative path-typed field resolves against the config directory; absolute paths are unchanged. So TOML `ssl_cert = "./server.crt"` is resolved automatically; INI keeps that same line as `./server.crt` verbatim. The piecemeal rule still applies to INI. Implementation note: the Rust crate can use one `Path` type and apply the resolution policy at the format-decoder layer, so the typed schema is identical between formats.
6. **`[port_*]` modeling.** In INI, keep the existing flat layout. In TOML, prefer `[port.<name>]` table-of-tables or keep `[port_<name>]` flat for parity?

   **Decision:** INI keeps flat layout (`[server]` + `[port_<name>]`) for compatibility. TOML uses table-of-tables: `server.ports = ["rpc_admin", "peer", "ws"]` plus `[port.rpc_admin]`, `[port.peer]`, etc. Internal Rust model is `BTreeMap<String, PortConfig>` with a parallel `Vec<String>` capturing source order. The `--convert-config` INI→TOML transform rewrites `[port_<name>]` to `[port.<name>]`.
7. **CLI overrides.** Confirm the proposed split: `ParsedConfig` (file only, immutable) + `CliOverrides` (CLI only) → `RuntimeConfig`. CLI fields and their types are listed in §2.2.

   **Decision:** Confirmed. `ParsedConfig` is immutable, file-only. `CliOverrides` is a mutable struct with one optional field per CLI-overridable knob (the 8 fields from §2.2). `RuntimeConfig` is the merged read facade: for each overridable field, its accessor returns the CLI value if set, otherwise the parsed value (`cli.X.unwrap_or(parsed.X)` shape). CLI-only fields (`FORCE_MULTI_THREAD`, anything else test-only) live on `CliOverrides` or a separate test-hook struct, never on `ParsedConfig`.
8. **`kSIZED_ITEMS` table.** Carry over verbatim or refactor (e.g. per-`SizedItem` typed default + per-`NodeSize` override)?

   **Decision:** Move to Rust along with the `NodeSize` auto-detection. The table itself is pure data (13×5 integers), the `static_assert` is just a sanity guard, and the OS RAM/CPU probe (`sysctl HW_MEMSIZE` / `sysinfo` / `GlobalMemoryStatusEx` and `available_parallelism`) has straightforward Rust equivalents. The Rust crate exposes:
   - A `NodeSize` enum (`Tiny|Small|Medium|Large|Huge`).
   - The `SizedItem` enum and the const table.
   - A `RuntimeConfig::sized_value(SizedItem) -> i32` accessor (or a typed method per item — exact shape decided in step 2).
   - The `NodeSize` auto-detection logic (RAM threshold walk + CPU cap).

   ~16 C++ call sites become FFI calls into Rust returning a primitive `int` — acceptable cost. Carry the table over **verbatim** (same 13 items, same 5 buckets, same values) — the refactor question (per-item typed defaults vs. flat table) is deferred to step 2 design.
9. **`validators.txt`.** Keep the "merge sections" model or expose as a dedicated typed sub-document?

   **Decision:** Splice model, matching existing behavior. The Rust parser reads `validators.txt` (resolved relative to the config dir per §5) as a second INI/TOML file and merges its `[validators]`, `[validator_keys]`, `[validator_list_sites]`, `[validator_list_keys]`, `[validator_list_threshold]` sections into the main config. One unified `ParsedConfig` is what consumers see; they don't need to know which file a section came from. In strict mode, overlap between the two files (same section appearing in both) is an error rather than a silent append.
10. **Silent clamp vs error.** `max_transactions` is silently clamped to `[100,1000]` today and `fetch_depth` to `≥ 10`. In strict mode: error or clamp? Recommend error.

   **Decision:** Format-asymmetric. INI keeps the existing silent-clamp behavior (`max_transactions` coerced to `[100,1000]`, `fetch_depth` floored at 10) so existing rippled.cfg files load unchanged. TOML is strict: out-of-range values fail parse with a clear error message. This pattern — *INI compat-first, TOML strict* — should be applied as the general rule whenever the two formats might diverge on edge-case behavior.
11. **Strict mode default.** Confirm step 2 ships with strict-by-default, `--check-config` and `--convert-config` as escape hatches.

   **Decision:** **Asymmetric defaults.** INI is **compat-first / lenient by default** — replicates existing parser behavior verbatim, including silent-ignore of unknown keys and unknown sections, and the per-field clamp/error rules from §5 of the validators table. TOML is **strict by default** — unknown keys, unknown sections, and out-of-range values are errors. INI may be tightened later if the community has migrated and the lenience is no longer needed. `--check-config` and `--convert-config` continue to ship in step 4 as migration helpers. This decision **supersedes** the plan doc's earlier "strict from day one" position; the plan doc should be updated.
12. **Unknown sub-keys in known sections.** Strict mode should reject unknown *keys* inside known sections (e.g. `sqlite.foobar`), not just unknown sections. Confirm?

   **Decision:** Asymmetric, per #11. INI silently ignores unknown keys (matches existing `BasicConfig` behavior: every parsed key sits in `lookup_`, only the keys consumers ask for get read). TOML rejects unknown keys with a `deny_unknown_fields`-style error and a useful "did you mean…?" message where feasible.
13. **Mutable `Config` during setup.** Today `Config::setup` writes the absolute `database_path` back into the parsed sections. In the new model, do we keep `ParsedConfig` immutable and put the resolved-path on `RuntimeConfig` instead?

   **Decision:** `ParsedConfig` is immutable. Resolved-path views (`database_path` absolutized, `debug_logfile` absolutized, `validators_file` absolutized — the three auto-resolved paths from #5) live on `RuntimeConfig` and are computed during construction. External observable behavior matches today: consumers that read the resolved path after setup get the absolute value.
14. **Side effects in setup.** `HTTPClient::initializeSSLContext`, `create_directories(dataDir)`, env-var reads, `validators.txt` ingest, `stderr` echo — should the Rust crate own these, or should they remain on the C++ side after Rust hands back a parsed value? Recommend: keep the *parser* pure; expose a separate `bootstrap()` step on the C++ side that performs filesystem and SSL bring-up. This keeps the Rust crate testable.

   **Decision:** Principle — *Rust does anything that doesn't require crossing back into C++.* Concretely:
   - **Rust:** Config file discovery via env vars (`HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`) and filesystem existence checks; `mkdir -p` of the resolved data directory; path absolutization; `validators.txt` parsing and section merge; `stderr` echo of the loaded config file path when not quiet; cross-field validation (e.g. `checkZeroPorts`, the `validation_seed`/`validator_token` XOR, `network_quorum` ≤ effective `peers_max`); the `RamSizeGb` / CPU probe for `NodeSize` auto-detection (per #8).
   - **C++:** `HTTPClient::initializeSSLContext` (uses C++ class types and Boost.Asio context — not worth bridging). Stays in a thin C++ bootstrap step that runs after `RuntimeConfig` is constructed.
   - **Testability:** the Rust crate is testable without C++ — every side effect it owns is either a stdlib filesystem/env call or pure data manipulation, so a fixture-based test rig is straightforward.
15. **`FORCE_MULTI_THREAD`.** Confirm this is *not* a config field and lives only as a test hook (probably a builder option on `Config` in tests).

   **Decision:** Not a file-backed config field, but a real runtime knob. Production reads it (`Application.cpp:329`); only tests write it. Sits on `CliOverrides` alongside the actual CLI overrides — same shape (post-parse, optional, no file backing). Not exposed as a `--force_multi_thread` CLI flag; the test harness constructs `CliOverrides { force_multi_thread: Some(true), .. }` directly. The umbrella struct may be renamed to something broader than `CliOverrides` (e.g. `ConfigOverrides`) to capture that not every field maps to a CLI flag.

Step 2 should begin once these are answered.

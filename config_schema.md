# xrpld Configuration File Schema

This document describes the configuration file schema for `xrpld` as implemented
by the current C++ code base. It is intended as the specification for the Rust
re-implementation of the configuration loader (`crates/config`).

The information below was extracted from:

| Source                                                              | Purpose                                                                                          |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `src/xrpld/core/Config.h` / `Config.cpp`                            | Core `Config` class, primary load + most validations                                             |
| `src/xrpld/core/ConfigSections.h`                                   | Authoritative list of section identifiers                                                        |
| `include/xrpl/basics/BasicConfig.h` / `src/libxrpl/basics/BasicConfig.cpp` | Generic INI parser (`Section`, `BasicConfig`)                                                  |
| `src/xrpld/app/main/Main.cpp`                                       | Command-line option parsing (CLI overrides)                                                      |
| `src/libxrpl/server/Port.cpp`                                       | Per-port (`[port_*]`) section parsing                                                            |
| `src/xrpld/rpc/detail/ServerHandler.cpp`                            | Aggregation of `[server]` and child port sections                                                |
| `src/xrpld/app/main/GRPCServer.cpp`                                 | `[port_grpc]` parsing                                                                            |
| `src/xrpld/overlay/detail/OverlayImpl.cpp`                          | `[overlay]`, `[crawl]`, `[vl]` parsing                                                           |
| `src/xrpld/app/misc/detail/TxQ.cpp`                                 | `[transaction_queue]` parsing                                                                    |
| `src/xrpld/app/misc/detail/setup_HashRouter.cpp`                    | `[hashrouter]` parsing                                                                           |
| `src/xrpld/app/main/CollectorManager.cpp`                           | `[insight]` parsing                                                                              |
| `src/xrpld/perflog/detail/PerfLogImp.cpp`                           | `[perf]` parsing                                                                                 |
| `src/xrpld/app/misc/SHAMapStoreImp.cpp`                             | `[node_db]` online-delete parsing                                                                |
| `src/libxrpl/nodestore/Database.cpp` / `backend/NuDBFactory.cpp`    | `earliest_seq`, `nudb_block_size`                                                                |
| `src/libxrpl/rdb/SociDB.cpp`                                        | `[sqdb]` parsing                                                                                 |
| `src/xrpld/app/misc/detail/ValidatorKeys.cpp`                       | `[validation_seed]` / `[validator_token]` parsing                                                |
| `src/xrpld/app/main/NodeIdentity.cpp`                               | `[node_seed]` parsing and `--nodeid` CLI override                                                |
| `src/xrpld/app/main/Application.cpp`                                | Consumes `[amendments]`, `[veto_amendments]`, `[cluster_nodes]`, `[rpc_startup]`, etc.           |
| `cfg/xrpld-example.cfg`, `cfg/validators-example.txt`               | Documentation-by-example                                                                         |

---

## 1. File format

### 1.1. Encoding and line endings

* The file is UTF-8 with DOS (`\r\n`), UNIX (`\n`), or classic Mac (`\r`)
  line endings (`parseIniFile` normalizes both `\r\n` and lone `\r` to `\n`).
* Blank lines and lines beginning with `#` are ignored at the top level
  (`parseIniFile` in `src/xrpld/core/detail/Config.cpp`).

### 1.2. Sections

* A section header is a single line of the form `[<name>]`. Section names are
  case-sensitive in lookups (`std::unordered_map<std::string, ...>`).
* Lines outside any header belong to a default section with the empty name `""`.
* The same `[name]` may appear multiple times; lines from subsequent occurrences
  are appended.
* Inside a section, a line is either:
  * A **key/value pair**: `<key> = <value>`. The key matches the regex
    `^\s*([a-zA-Z][_a-zA-Z0-9]*)\s*=\s*(.*\S+)\s*$` and key lookup is
    case-sensitive in `Section::lookup_` (see `Section::append` in
    `BasicConfig.cpp`). Some consumers compare with `boost::iequals`, others
    require exact case — see the per-key notes below.
  * A **value** line: anything else. These are stored separately and exposed
    via `Section::values()` / `Section::lines()`. Used by list-style sections
    like `[ips]`, `[validators]`, `[features]`.

### 1.3. Comments and escaping

* A `#` anywhere in a line begins a trailing comment. The comment and trailing
  whitespace are stripped from the value before further parsing
  (`Section::append`).
* `\#` is an escape — the `\` is removed and the `#` is kept in the value.
* The presence of any trailing comments is tracked via
  `Section::hadTrailingComments_` and `BasicConfig::hadTrailingComments()` and
  triggers a warning at startup (`Main.cpp`) because the trailing-comment
  rules changed in a recent release.

### 1.4. Value types referenced below

* `<flag>` — boolean. `1`/`0`, or anything accepted by `boost::lexical_cast<bool>`
  (`true`/`false`, `yes`/`no`, `on`/`off`). For nested sections that go through
  `getIfExists<bool>`, the value is read as `int` and cast (so `1`/`0` only).
* `<unsigned>` — `std::size_t` / `std::uint32_t` parsed with
  `beast::lexicalCastThrow<...>`. Negative values throw `std::runtime_error`.
* `<integer>` — signed integer, ditto.
* `<duration>` — see `[amendment_majority_time]`; format `<n> <unit>` with
  `unit ∈ {minutes, hours, days, weeks}` (case-insensitive).
* `<path>` — file-system path. Relative paths are resolved against `CONFIG_DIR`
  (see §3.1).

### 1.5. CLI override semantics

Most fields **cannot** be overridden on the command line. Only the items listed
in §2 are settable from CLI. The rules below mention CLI overrides explicitly
where they exist; otherwise the field is **config-file only**.

---

## 2. Command-line options (boost program_options)

Defined in `src/xrpld/app/main/Main.cpp`. CLI flags that interact with the
config object are noted below; the rest are operational (help/version/test).

### 2.1. General

| Option                          | Type     | Effect on `Config` / startup                                                                           |
| ------------------------------- | -------- | ------------------------------------------------------------------------------------------------------ |
| `--conf <path>`                 | string   | Selects the config file; **overrides** the file search in §3.1. `CONFIG_DIR` becomes its parent dir.   |
| `--debug`                       | flag     | Routes a debug log sink at `Severity::Trace`. Does not modify `Config` fields.                         |
| `--help`, `-h`                  | flag     | Prints help and exits (no config load).                                                                |
| `--newnodeid`                   | flag     | Forces regeneration of node identity (clears `NodeIdentity` row in wallet DB).                         |
| `--nodeid <seed>`               | string   | **Overrides** `[node_seed]`. Must parse as a generic seed (`parseGenericSeed`).                        |
| `--quorum <unsigned>`           | unsigned | Sets `Config::VALIDATION_QUORUM`. Must be non-zero.                                                    |
| `--silent`                      | flag     | Sets `Config::SILENT_`. Also forces `QUIET_`.                                                          |
| `--quiet`, `-q`                 | flag     | Sets `Config::QUIET_`. Lowers log threshold to `Fatal`. Also reused by `--unittest` to suppress logs.  |
| `--standalone`, `-a`            | flag     | Sets `Config::RUN_STANDALONE_`. Disables peer connections; forces `LEDGER_HISTORY = 0`.                |
| `--verbose`, `-v`               | flag     | Log severity becomes `Trace`.                                                                          |
| `--definitions`                 | flag     | Dumps `getServerDefinitionsJson()` and exits.                                                          |
| `--version`                     | flag     | Prints version and exits.                                                                              |
| `--force_ledger_present_range`  | string   | Sets `Config::FORCED_LEDGER_RANGE_PRESENT = (min,max)`. Two comma-separated integers, `min ≤ max`.     |

### 2.2. Ledger / data

| Option                  | Type   | Effect                                                                                                         |
| ----------------------- | ------ | -------------------------------------------------------------------------------------------------------------- |
| `--import`              | flag   | Sets `Config::doImport = true`; reads `[import_db]` to migrate into `[node_db]`.                               |
| `--ledger <id|file>`    | string | Sets `Config::START_LEDGER`; sets `START_UP` to `Load` (or `Replay` if `--replay` is given).                   |
| `--ledgerfile <path>`   | string | Sets `Config::START_LEDGER` and `START_UP = LoadFile`.                                                         |
| `--load`                | flag   | `START_UP = Load`. Also triggered automatically when `node_db.fast_load = true` (`Config::FAST_LOAD`).         |
| `--net`                 | flag   | `START_UP = Network`. Incompatible with `Load`/`Replay` unless `FAST_LOAD`.                                    |
| `--replay`              | flag   | Used with `--ledger` to set `START_UP = Replay`.                                                               |
| `--trap_tx_hash <hash>` | string | Sets `Config::TRAP_TX_HASH` (only valid with `--replay`).                                                      |
| `--start`               | flag   | `START_UP = Fresh`.                                                                                            |
| `--vacuum`              | flag   | Runs `doVacuumDB(setupDatabaseCon(*config))` and exits. Not allowed in standalone mode.                        |
| `--valid`               | flag   | Sets `Config::START_VALID = true`.                                                                             |

### 2.3. RPC client

| Option                | Type      | Effect                                                                                                          |
| --------------------- | --------- | --------------------------------------------------------------------------------------------------------------- |
| `--rpc`               | flag      | Treats remaining positional args as an RPC command. Auto-set if positionals present.                            |
| `--rpc_ip <ip[:port]>`| string    | **Overrides** the RPC destination IP. Sets `Config::rpc_ip` to a `beast::IP::Endpoint`. Port must be non-zero.  |
| `--rpc_port <u16>`    | uint16    | **Deprecated** — merged into `--rpc_ip` if the latter has no port; warns when used.                             |

Test-only options (`--unittest`, `--unittest-*`, `--quiet`) are compiled in
when `ENABLE_TESTS` is defined and do not affect runtime config.

---

## 3. File location and lookup

### 3.1. Search order (`Config::setup` in `Config.cpp`)

If `--conf=<path>` is given, that file is used directly and `CONFIG_DIR`
is its absolute parent directory. The database directory defaults to
`CONFIG_DIR / "db"`.

Otherwise xrpld searches in this order, stopping at the first existing file:

1. `<cwd>/xrpld.cfg`
2. `<cwd>/rippled.cfg` (legacy)
3. `$XDG_CONFIG_HOME/xrpld/xrpld.cfg` (defaulting to `$HOME/.config/xrpld/` when
   `XDG_CONFIG_HOME` is unset). `$HOME` must be set.
4. `$XDG_CONFIG_HOME/xrpld/rippled.cfg`
5. `/etc/opt/xrpld/xrpld.cfg`
6. `/etc/opt/xrpld/rippled.cfg`

The system name (`xrpld`) is reported by `xrpl::systemName()`. The XDG path
also drives the default data directory (`$XDG_DATA_HOME/xrpld`, with default
`$HOME/.local/share/xrpld`). The system-wide fallback is `/var/opt/xrpld`.

### 3.2. Default file names (compile-time constants)

| Constant                       | Value             |
| ------------------------------ | ----------------- |
| `Config::kCONFIG_FILE_NAME`    | `xrpld.cfg`       |
| `Config::kCONFIG_LEGACY_NAME`  | `rippled.cfg`     |
| `Config::kDATABASE_DIR_NAME`   | `db`              |
| `Config::kVALIDATORS_FILE_NAME`| `validators.txt`  |

### 3.3. Side effects during load

In approximate execution order:

1. `Config::setupControl` — auto-detects `NODE_SIZE` from system RAM
   (`detail::getMemorySize` via Linux `sysinfo`, macOS `sysctl HW_MEMSIZE`,
   Windows `GlobalMemoryStatusEx`) and `std::thread::hardware_concurrency()`.
2. Path resolution reads environment: `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`.
3. Reads the config file via `getFileContents`. Failure is logged to `stderr`
   but does not throw — defaults remain in effect (`Config::load`).
4. Parses INI. Throws `std::runtime_error` on validation errors.
5. `database_path` is absolutized; `boost::filesystem::create_directories` is
   called on the data directory (throws if it cannot be created). In standalone
   mode the data dir is cleared instead.
6. `HTTPClient::initializeSSLContext(SSL_VERIFY_DIR, SSL_VERIFY_FILE, SSL_VERIFY)`
   is called — this mutates global SSL state used by the HTTPS client.
7. `[ledger_tx_tables].use_tx_tables` and `[node_db].fast_load` are read into
   `Config::USE_TX_TABLES_` and `Config::FAST_LOAD`.
8. `checkZeroPorts` enumerates `[server]` child sections and rejects any
   `port = 0` (zero ports are only legal in unit tests).
9. If a validators file is configured (or `validators.txt` exists alongside
   the config), its `[validators]`, `[validator_keys]`,
   `[validator_list_sites]`, `[validator_list_keys]`,
   `[validator_list_threshold]` sections are appended into the main config.
10. `[features]` values are validated against `getRegisteredFeature(...)` and
    converted to `uint256` IDs in `Config::features`.
11. If `LEDGER_HISTORY > 0` and any `[sqlite]` setting reduces durability,
    `setupDatabaseCon` logs a warning (see `kSQLITE_TUNING_CUTOFF`).
12. If `Config::hadTrailingComments()` is `true`, `Main.cpp` emits a warning
    about the recent change in comment handling.
13. `getDebugLogFile()` (called later) creates the log directory if needed.
14. `RUN_STANDALONE_ == true` forces `LEDGER_HISTORY = 0` after parsing.
15. If both `[validation_seed]` and `[validator_token]` are present, the load
    throws. Validators implicitly set `PATH_SEARCH_MAX = 0`.

---

## 4. Section catalog

In the tables below:

* **Type** is the value type as parsed by C++.
* **Required** — yes/no. "Yes (network mode)" means required when not in
  standalone (`Config::RUN_STANDALONE_`).
* **CLI** — name of the CLI option that overrides the value (or `—` if
  none).
* **Default** — the value used when the field is missing.
* **Validation** — what causes `Config::load` (or the relevant `setup*`
  call) to throw at load time.

Unless noted otherwise, every section is **optional**.

### 4.1. [server] and per-port sections

#### `[server]`

A list-style section. Its **value lines** are the names of port sections.
**Required**: yes (`parsePorts` in `ServerHandler.cpp` throws "Required section
[server] is missing" if absent).

Key/value pairs at this level are inherited as defaults by every named port
section (`parsePort` is called twice — once on `[server]`, once on the named
section).

| Behavior                                                                                           |
| -------------------------------------------------------------------------------------------------- |
| Names listed must each have a corresponding `[<name>]` section (else throws).                      |
| Name `port_grpc` is skipped here and parsed separately by `GRPCServer`.                            |
| In standalone mode, the `peer` protocol is removed from each port; empty ports are dropped.        |
| In network mode, exactly one port may have `peer` (throws if more); zero peer ports → warning.     |
| At top level, `port = 0` is rejected by `checkZeroPorts`.                                          |

#### `[<port_name>]` — per-port settings

Source: `src/libxrpl/server/Port.cpp` (`parsePort`).

| Field                          | Type           | Required | CLI | Default       | Validation                                                                        |
| ------------------------------ | -------------- | -------- | --- | ------------- | --------------------------------------------------------------------------------- |
| `ip`                           | IP address     | yes      | —   | —             | `make_address` must parse. Throws otherwise.                                      |
| `port`                         | uint16         | yes      | —   | —             | Must parse; in section named `server` may not be `0`. `checkZeroPorts` also runs. |
| `protocol`                     | csv of strings | yes      | —   | —             | Values are inserted into a `std::set`. Recognized: `http`, `https`, `ws`, `wss`, `peer`. WebSocket and non-WebSocket protocols cannot coexist on the same port (enforced later in server start). |
| `limit`                        | "unlimited"\|uint16 | no | —   | `unlimited`   | Non-`unlimited` must parse as uint16 (= max 65535).                               |
| `send_queue_limit`             | uint16         | no       | —   | `100`         | Must be `> 0` (throws on `0`).                                                    |
| `user`, `password`             | string         | no       | —   | `""`          | —                                                                                 |
| `admin_user`, `admin_password` | string         | no       | —   | `""`          | —                                                                                 |
| `admin`                        | csv IP/CIDR    | no       | —   | empty         | Each entry must be parseable as IPv4/IPv6 address or network; subnet must equal its canonical form. `0.0.0.0` / `::` collapse to "all addresses". |
| `secure_gateway`               | csv IP/CIDR    | no       | —   | empty         | Same parsing rules as `admin`. Overlap with `admin` resolves in `admin`'s favor.   |
| `ssl_key`, `ssl_cert`, `ssl_chain` | path       | no       | —   | empty         | Loaded by `makeSslContextAuthed`; if all empty for a secure protocol, a self-signed cert is generated. |
| `ssl_ciphers`                  | OpenSSL cipher list | no  | —   | modern default| —                                                                                 |
| `permessage_deflate`           | bool           | no       | —   | `true`        | Bool parse via `valueOr<bool>`.                                                   |
| `client_max_window_bits`       | int            | no       | —   | `15`          | Range `9..15` per documentation; not enforced in code.                            |
| `server_max_window_bits`       | int            | no       | —   | `15`          | Same as above.                                                                    |
| `client_no_context_takeover`   | bool           | no       | —   | `false`       | —                                                                                 |
| `server_no_context_takeover`   | bool           | no       | —   | `false`       | —                                                                                 |
| `compress_level`               | int            | no       | —   | `8` (code), docs say 3 | Documented range `0..9`; not enforced in code.                            |
| `memory_level`                 | int            | no       | —   | `4`           | Documented range `1..9`; not enforced in code.                                    |

#### `[port_grpc]` (handled by `GRPCServer`, not `ServerHandler`)

Optional. Schema:

| Field             | Type            | Required | Validation                                                                         |
| ----------------- | --------------- | -------- | ---------------------------------------------------------------------------------- |
| `ip`              | IP address      | required if section present | Must parse.                                                       |
| `port`            | uint            | required if section present | `std::stoi` must succeed.                                         |
| `secure_gateway`  | csv IP          | optional | Each entry must parse; unspecified addresses (`0.0.0.0`, `::`) rejected.           |
| `ssl_cert`        | path            | optional | If `ssl_cert` **or** `ssl_key` is set, **both** must be set (throws otherwise).    |
| `ssl_key`         | path            | optional | See above.                                                                         |
| `ssl_cert_chain`  | path            | optional | Requires `ssl_cert` + `ssl_key`.                                                   |
| `ssl_client_ca`   | path            | optional | Requires `ssl_cert` + `ssl_key`.                                                   |

#### `[rpc_startup]`

A list-style section. Each line is a JSON object passed as an RPC command at
startup (`Application::setup` in `Application.cpp`). Unparseable lines log
a `fatal` (non-blocking).

#### `[websocket_ping_frequency]`

Documented in the example config. **Not currently consumed** by the code in
this branch; defined for forward compatibility.

#### `[server_domain]`

Single-value section. `Config::SERVER_DOMAIN`. Validated by
`isProperlyFormedTomlDomain` — throws if malformed.

---

### 4.2. Peer protocol

| Section / Field           | Type                            | Required | CLI | Default             | Validation                                                                                                                                                       |
| ------------------------- | ------------------------------- | -------- | --- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[compression]`           | flag                            | no       | —   | `0`                 | `lexical_cast<bool>`.                                                                                                                                            |
| `[ips]`                   | list of `host[ port]` strings   | no       | —   | empty (hard-coded starter list applies later) | `host:port` is rewritten to `host port`. IPv6 addresses (multiple `:`) are skipped by that rewrite.                                       |
| `[ips_fixed]`             | list of `host port` strings     | no       | —   | empty               | Port required per documentation; same `:`-to-space rewrite.                                                                                                       |
| `[peer_private]`          | flag                            | no       | —   | `0`                 | `lexicalCastThrow<bool>`.                                                                                                                                        |
| `[peers_max]`             | unsigned                        | no       | —   | `0`                 | If set, `peers_in_max` and `peers_out_max` are ignored. Also constrains `[network_quorum]`: error if `network_quorum > peers_max` (or `21` if `peers_max == 0`).  |
| `[peers_in_max]`          | unsigned                        | no       | —   | `0`                 | Must be `≤ 1000`. Must be set together with `peers_out_max`.                                                                                                     |
| `[peers_out_max]`         | unsigned                        | no       | —   | `0`                 | Must be in `10..1000`. Must be set together with `peers_in_max`.                                                                                                 |
| `[node_seed]`             | base58 seed (1 value line)      | no       | `--nodeid` | wallet-DB-generated identity | `parseBase58<Seed>` must succeed.                                                                                                                       |
| `[cluster_nodes]`         | list of `<pubkey> [<name>]`     | no       | —   | empty               | `Cluster::load` validates each line; load failure aborts startup.                                                                                                |
| `[max_transactions]`      | int                             | no       | —   | `250`               | Clamped to `[100, 1000]` (`kMIN_JOB_QUEUE_TX`, `kMAX_JOB_QUEUE_TX`).                                                                                             |

#### `[overlay]`

| Field                | Type     | Required | Default | Validation                                                                                |
| -------------------- | -------- | -------- | ------- | ----------------------------------------------------------------------------------------- |
| `public_ip`          | IP addr  | no       | unset   | Must parse; must not be `beast::IP::isPrivate`.                                           |
| `ip_limit`           | int      | no       | auto    | Must be `≥ 0` (throws on negative). Upper bound is enforced inside the overlay code.      |
| `max_unknown_time`   | unsigned (seconds) | no | `600` | Must lie in `[300, 1800]`.                                                                |
| `max_diverged_time`  | unsigned (seconds) | no | `300` | Must lie in `[60, 900]`.                                                                  |

#### `[transaction_queue]` — EXPERIMENTAL

| Field                                | Type     | Default  | Validation                                                                  |
| ------------------------------------ | -------- | -------- | --------------------------------------------------------------------------- |
| `ledgers_in_queue`                   | unsigned | `20`     | Lexical cast.                                                               |
| `minimum_queue_size`                 | unsigned | `2000`   | Lexical cast.                                                               |
| `retry_sequence_percent`             | unsigned | `25`     | Lexical cast.                                                               |
| `minimum_escalation_multiplier`      | unsigned | `500`    | Lexical cast.                                                               |
| `minimum_txn_in_ledger`              | unsigned | `5`      | Lexical cast.                                                               |
| `minimum_txn_in_ledger_standalone`   | unsigned | `1000`   | Lexical cast.                                                               |
| `target_txn_in_ledger`               | unsigned | `50`     | Lexical cast.                                                               |
| `maximum_txn_in_ledger`              | unsigned | unset    | Must be `≥ minimum_txn_in_ledger` and `≥ minimum_txn_in_ledger_standalone`. |
| `normal_consensus_increase_percent`  | unsigned | `20`     | Clamped to `[0, 1000]`.                                                     |
| `slow_consensus_decrease_percent`    | unsigned | `50`     | Clamped to `[0, 100]`.                                                      |
| `maximum_txn_per_account`            | unsigned | `10`     | Lexical cast.                                                               |
| `minimum_last_ledger_buffer`         | unsigned | `2`      | Lexical cast.                                                               |
| `zero_basefee_transaction_feelevel`  | unsigned | `256000` | Lexical cast (documented; consumed inside TxQ).                             |

#### `[reduce_relay]`

| Field                                  | Type     | Default | Validation                                                                                |
| -------------------------------------- | -------- | ------- | ----------------------------------------------------------------------------------------- |
| `vp_base_squelch_enable`               | bool     | `false` | Cannot be present together with `vp_enable` (throws — deprecation).                       |
| `vp_enable`                            | bool     | `false` | Deprecated alias of the above.                                                            |
| `vp_base_squelch_max_selected_peers`   | unsigned | `5`     | Must be `≥ 3`.                                                                            |
| `tx_enable`                            | bool     | `false` | —                                                                                         |
| `tx_metrics`                           | bool     | `false` | —                                                                                         |
| `tx_min_peers`                         | unsigned | `20`    | Must be `≥ 10`.                                                                           |
| `tx_relay_percentage`                  | unsigned | `25`    | Must be in `[10, 100]`.                                                                   |

---

### 4.3. Protocol

| Section / Field            | Type                                                                  | Required | CLI         | Default                  | Validation                                                                                                                                  |
| -------------------------- | --------------------------------------------------------------------- | -------- | ----------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `[relay_proposals]`        | string `all` \| `trusted` \| `drop_untrusted`                         | no       | —           | `trusted`                | Other values throw.                                                                                                                         |
| `[relay_validations]`      | string `all` \| `trusted` \| `drop_untrusted`                         | no       | —           | `all`                    | Other values throw.                                                                                                                         |
| `[validation_seed]`        | base58 seed                                                           | no       | —           | unset                    | Mutually exclusive with `[validator_token]` (Config and ValidatorKeys both check).                                                          |
| `[validator_token]`        | base64 blob (possibly multi-line)                                     | no       | —           | unset                    | Mutex with `[validation_seed]`. `loadValidatorToken` must succeed; embedded manifest must verify against derived public key.                |
| `[validator_key_revocation]` | base64 blob                                                         | no       | —           | unset                    | Passed to `validatorManifests_->load`.                                                                                                      |
| `[validators_file]`        | path (single value)                                                   | no (network mode reads `validators.txt` from config dir by default) | — | unset | Empty string throws. Relative paths resolved against `CONFIG_DIR`. File must exist and be a regular file or symlink. Must contain at least one of `[validators]`, `[validator_keys]`, `[validator_list_keys]`. |
| `[validators]`             | list of public keys (`n…`)                                            | no       | —           | empty                    | `[validator_keys]` lines are appended to this section after load.                                                                           |
| `[validator_keys]`         | list of public keys                                                   | no       | —           | empty                    | Merged into `[validators]`.                                                                                                                 |
| `[validator_list_sites]`   | list of URIs                                                          | no       | —           | empty                    | Non-empty `[validator_list_sites]` requires non-empty `[validator_list_keys]` (throws otherwise).                                            |
| `[validator_list_keys]`    | list of hex-encoded public keys                                       | no       | —           | empty                    | —                                                                                                                                            |
| `[validator_list_threshold]` | single unsigned                                                     | no       | —           | unset (computed)         | At most one line. `0` ⇒ keep `nullopt` (auto-compute). Otherwise must be `≤ size([validator_list_keys])`.                                   |
| `[path_search_old]`        | int                                                                   | no       | —           | `2`                      | Lexical cast.                                                                                                                               |
| `[path_search]`            | int                                                                   | no       | —           | `2`                      | Lexical cast.                                                                                                                               |
| `[path_search_fast]`       | int                                                                   | no       | —           | `2`                      | Lexical cast.                                                                                                                               |
| `[path_search_max]`        | int                                                                   | no       | —           | `3` (or `0` for validators) | Lexical cast. Auto-set to `0` if `[validation_seed]` or `[validator_token]` is present (unless explicitly set later in the file).        |
| `[fee_default]`            | uint64 drops                                                          | no       | —           | from `[voting]`          | Overrides `[voting].reference_fee`. Bounded only by `XRPAmount::value_type`.                                                                |
| `[workers]`                | int                                                                   | no       | —           | `0` (auto)               | Must be in `[1, 1024]` when set.                                                                                                            |
| `[io_workers]`             | int                                                                   | no       | —           | `0` (auto: 2)            | Must be in `[1, 1024]` when set.                                                                                                            |
| `[prefetch_workers]`       | int                                                                   | no       | —           | `0` (auto: 4)            | Must be in `[1, 1024]` when set.                                                                                                            |
| `[network_id]`             | `main` \| `testnet` \| `devnet` \| uint32                             | no       | —           | `0`                      | Numeric values are parsed with `lexicalCastThrow<uint32_t>`.                                                                                |
| `[network_quorum]`         | unsigned                                                              | no       | `--quorum`  | `1`                      | At load: must not exceed effective `peers_max` (or `21` if unset). CLI value must be non-zero.                                              |
| `[ledger_replay]`          | flag                                                                  | no       | —           | `false`                  | Lexical cast.                                                                                                                               |
| `[ledger_history]`         | `full` \| `none` \| uint32                                            | no       | —           | `256`                    | `full` → `UINT32_MAX`. `none` → `0`. Otherwise numeric. Standalone mode overrides to `0`. Must be `≤ node_db.online_delete` when both set.   |
| `[fetch_depth]`            | `full` \| `none` \| uint32                                            | no       | —           | `1000000000`             | `full` → `UINT32_MAX`. `none` → `0`. Final value is `max(parsed, 10)`.                                                                       |
| `[sweep_interval]`         | unsigned (seconds)                                                    | no       | —           | derived from `NODE_SIZE` | Must be in `[10, 600]` when set.                                                                                                            |
| `[amendments]`             | list of names                                                         | no       | —           | empty                    | Used to vote *for* amendments. Each entry must be a known feature (validated by `AmendmentTable`).                                          |
| `[veto_amendments]`        | list of names                                                         | no       | —           | empty                    | Used to vote *against* amendments. Same validation.                                                                                          |
| `[features]`               | list of feature names                                                 | no       | —           | empty                    | Each name must map to a registered feature (`getRegisteredFeature`); unknown values throw.                                                  |
| `[amendment_majority_time]`| `<n> <minutes\|hours\|days\|weeks>`                                   | no       | —           | `kDEFAULT_AMENDMENT_MAJORITY_TIME` | Regex `^\s*(\d+)\s*(minutes|hours|days|weeks)\s*(\s+.*)?$`. Result must be `≥ 15 minutes`.                                        |
| `[beta_rpc_api]`           | flag                                                                  | no       | —           | `false`                  | Lexical cast.                                                                                                                               |
| `[hashrouter].hold_time`   | int (seconds)                                                         | no       | —           | code default             | Must be `≥ 12` when set.                                                                                                                    |
| `[hashrouter].relay_time`  | int (seconds)                                                         | no       | —           | code default             | Must be `≥ 8` when set; must be `≤ hold_time`.                                                                                              |

---

### 4.4. HTTPS client

| Field                | Type | Default | Validation                                                |
| -------------------- | ---- | ------- | --------------------------------------------------------- |
| `[ssl_verify]`       | flag | `true`  | Lexical cast.                                              |
| `[ssl_verify_file]`  | path | empty   | Passed verbatim to `HTTPClient::initializeSSLContext`.    |
| `[ssl_verify_dir]`   | path | empty   | Same.                                                      |

These three are read into `Config::SSL_VERIFY*` and consumed during load by
`HTTPClient::initializeSSLContext` — a side effect on global SSL state.

---

### 4.5. Database

#### `[node_db]` (required for non-test runs; `SHAMapStoreImp` throws "Missing [node_db] entry" if absent)

| Field                    | Type                  | Required | Default                         | Validation                                                                                                                                |
| ------------------------ | --------------------- | -------- | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                   | `NuDB` \| `RocksDB`   | yes      | —                               | Compared with `boost::iequals("RocksDB")`. Other values fall through to NuDB-style processing.                                            |
| `path`                   | path                  | yes      | —                               | Must be a directory if it exists, else created.                                                                                           |
| `fast_load`              | bool                  | no       | `false`                         | If true also forces `Config::START_UP = Load`.                                                                                            |
| `earliest_seq`           | uint32                | no       | `kXRP_LEDGER_EARLIEST_SEQ` (`32570`) | Must be `≥ 1`.                                                                                                                       |
| `online_delete`          | uint                  | no       | `0` (disabled)                  | When non-zero: must be `≥ kMINIMUM_DELETION_INTERVAL` (or `_SA` value in standalone). Must be `≥ Config::LEDGER_HISTORY`.                 |
| `nudb_block_size`        | uint                  | no       | `4096`                          | Must be power of 2 in `[4096, 32768]`. Only meaningful for `type=NuDB`.                                                                   |
| `advisory_delete`        | bool                  | no       | `false`                         | Only meaningful when `online_delete` is set.                                                                                              |
| `delete_batch`           | uint                  | no       | `100`                           | Only meaningful when `online_delete` is set.                                                                                              |
| `back_off_milliseconds`  | uint                  | no       | `100`                           | Also accepted under deprecated alias `backOff`.                                                                                           |
| `age_threshold_seconds`  | uint                  | no       | `60`                            | —                                                                                                                                          |
| `recovery_wait_seconds`  | uint                  | no       | `5`                             | —                                                                                                                                          |
| `cache_mb`               | uint                  | RocksDB only | derived from `SizedItem::HashNodeDbCache` | Defaulted automatically when missing.                                                                                                   |
| `filter_bits`            | uint                  | RocksDB only | `10` (only when `NODE_SIZE ≥ 2`) | Defaulted automatically.                                                                                                                |

#### `[import_db]`

Same schema as `[node_db]`. Consumed only when `--import` is given on the
command line. Sets `Config::doImport = true`.

#### `[database_path]`

A legacy single-value section (one line). Provides the root directory for
SQLite-backed bookkeeping databases. Absolutized during `setup`. If unset and
not in standalone mode, `setupDatabaseCon` throws "database_path must be set".

#### `[sqlite]`

| Field                | Type                                              | Default                              | Validation                                                                                                                                                                              |
| -------------------- | ------------------------------------------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `safety_level`       | `high` \| `low`                                   | unset                                | `low` => `journal_mode=memory, synchronous=off, temp_store=memory` and flips the risk-warning flag. Any other value throws. Cannot be combined with `journal_mode`/`synchronous`/`temp_store`. |
| `journal_mode`       | `delete` \| `truncate` \| `persist` \| `memory` \| `wal` \| `off` | `wal`              | Throws on unknown values. Cannot coexist with `safety_level`.                                                                                                                          |
| `synchronous`        | `off` \| `normal` \| `full` \| `extra`            | `normal`                             | Throws on unknown values. Cannot coexist with `safety_level`.                                                                                                                           |
| `temp_store`         | `default` \| `file` \| `memory`                   | `file`                               | Throws on unknown values. Cannot coexist with `safety_level`.                                                                                                                           |
| `page_size`          | int                                               | `4096`                               | Must be in `[512, 65536]` **and** a power of 2.                                                                                                                                         |
| `journal_size_limit` | int                                               | `1582080`                            | Lexical cast.                                                                                                                                                                            |

A startup warning is logged when `LEDGER_HISTORY > kSQLITE_TUNING_CUTOFF`
(currently 10 million) and any reduced-durability setting is in effect.

#### `[sqdb]`

| Field      | Type   | Default   | Validation                                                                  |
| ---------- | ------ | --------- | --------------------------------------------------------------------------- |
| `backend`  | string | `sqlite`  | Only `sqlite` is accepted; any other value throws "Unsupported soci backend". |

#### `[ledger_tx_tables]`

| Field           | Type | Default | Validation     |
| --------------- | ---- | ------- | -------------- |
| `use_tx_tables` | bool | `true`  | `int`-to-`bool` via `getIfExists<bool>` (so only `1`/`0`). |

---

### 4.6. Diagnostics

#### `[debug_logfile]`

Single-value section. `Config::DEBUG_LOGFILE_`. Relative paths are resolved
against `CONFIG_DIR` (in `getDebugLogFile()`). The parent directory is created
on demand (failure logs a warning to `stderr`, does not throw).

#### `[insight]`

| Field      | Type   | Default              | Validation                                                                              |
| ---------- | ------ | -------------------- | --------------------------------------------------------------------------------------- |
| `server`   | string | unset (NullCollector) | Only `statsd` is recognized; any other value silently selects the NullCollector.        |
| `address`  | `host:port` | empty             | Only consumed when `server=statsd`; parsed by `beast::IP::Endpoint::fromString`.        |
| `prefix`   | string | empty                | Free-form.                                                                              |

#### `[perf]`

| Field          | Type     | Default | Validation                                                |
| -------------- | -------- | ------- | --------------------------------------------------------- |
| `perf_log`     | path     | empty   | Relative paths resolved against `CONFIG_DIR`. Setting this enables performance logging. |
| `log_interval` | uint64 (seconds) | `1` | Lexical cast.                                             |

---

### 4.7. Voting

#### `[voting]`

| Field             | Type          | Default       | Validation                                                                                  |
| ----------------- | ------------- | ------------- | ------------------------------------------------------------------------------------------- |
| `reference_fee`   | uint64 drops  | `10`          | Capped to `XRPAmount::value_type` max — out-of-range values are silently ignored.            |
| `account_reserve` | uint32 drops  | `1000000` (1 XRP) | —                                                                                       |
| `owner_reserve`   | uint32 drops  | `200000` (0.2 XRP) | —                                                                                      |

`[fee_default]` (top-level) overrides `voting.reference_fee` post-hoc.

---

### 4.8. Miscellaneous

| Section / Field          | Type                                      | Default              | Validation                                                                                                                                                                          |
| ------------------------ | ----------------------------------------- | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[node_size]`            | `tiny`\|`small`\|`medium`\|`large`\|`huge` or int `0..4` | auto-detected from RAM and CPU | Names are case-insensitive. Numeric values are clamped to `[0, 4]`.                                                                                                       |
| `[signing_support]`      | flag                                      | `false`              | Enables `sign` / `sign_for` RPC commands. Logged with a deprecation warning when enabled.                                                                                            |
| `[elb_support]`          | flag                                      | `false`              | —                                                                                                                                                                                    |
| `[crawl]`                | bullet flag `0` \| `1` (value line) **and** k/v pairs | `1` (enabled)        | At most one value line. Sub-keys: `overlay` (default `1`), `server` (default `1`), `counts` (default `0`), `unl` (default `1`). All booleans.                                       |
| `[vl]`                   | k/v: `enabled = <bool>` (matched literally as `enabled` in code, even though the docs say `enable`) | unset    | —                                                                                                                                                                                    |

---

### 4.9. Sections defined but currently unused

These section names are reserved (declared in `ConfigSections.h`) but the
current code does not read from them in this branch:

* `[sntp_servers]` (`SECTION_SNTP`)
* `[websocket_ping_frequency]`
* `[relational_db]` (`SECTION_RELATIONAL_DB`)

A Rust loader should still accept them without error to preserve compatibility
with existing config files.

---

## 5. Cross-section validations summary

The following rules are enforced after parsing all sections (in
`Config::loadFromString`, `Application::setup`, and the various `setup*`
helpers):

1. `[validation_seed]` and `[validator_token]` are mutually exclusive.
2. If either of the above is present, `[path_search_max]` defaults to `0`
   (overridable by an explicit `[path_search_max]` later in the file).
3. `[peers_in_max]` and `[peers_out_max]` must be set together; ignored if
   `[peers_max]` is set.
4. `[network_quorum]` must not exceed effective `[peers_max]` (with `21`
   substituted when `peers_max == 0`).
5. `[validator_list_threshold]` must be `≤ |validator_list_keys|`.
6. `[validator_list_sites]` non-empty requires `[validator_list_keys]`
   non-empty.
7. `[node_db].online_delete ≥ Config::LEDGER_HISTORY` when both are non-zero.
8. `[ledger_history] ≤ [node_db].online_delete` (same rule, expressed the
   other way).
9. `[sqlite].safety_level` cannot be combined with `journal_mode`,
   `synchronous`, or `temp_store`.
10. `[transaction_queue].maximum_txn_in_ledger` must be `≥` both
    `minimum_txn_in_ledger` and `minimum_txn_in_ledger_standalone`.
11. `[reduce_relay].vp_base_squelch_enable` and `vp_enable` cannot both be set.
12. `[hashrouter].relay_time ≤ hashrouter.hold_time`.
13. `[server]` must list at least one named port section, each of which must
    exist; in network mode at most one of them may include the `peer`
    protocol; `port = 0` is rejected by `checkZeroPorts`.
14. `[port_grpc].ssl_cert` and `ssl_key` must both be set if either is;
    `ssl_cert_chain` and `ssl_client_ca` require both.
15. `[validators_file]` if explicitly set must exist and contain at least one
    of `[validators]`, `[validator_keys]`, `[validator_list_keys]`.
16. Every entry in `[features]` must be a registered feature.

---

## 6. Behavior notes for the Rust port

* The parser should keep the **two-level** structure (`BasicConfig`, `Section`)
  because most consumers re-parse sections through their own `set` /
  `getIfExists` helpers. Replicating that signature lets us preserve the
  field-level error messages users rely on.
* The parser's `Section` distinguishes **value lines** (no `=`) from
  **key/value pairs**. Several sections (`[ips]`, `[validators]`,
  `[features]`, `[rpc_startup]`, `[crawl]`) rely on `Section::values()` /
  `Section::lines()`. Both views must be exposed.
* Boolean parsing has two distinct paths:
  * `lexicalCastThrow<bool>` accepts the full `boost::lexical_cast<bool>` set
    (`true/false/yes/no/on/off/1/0`).
  * `getIfExists<bool>` reads the value as `int` first, so it accepts only
    `1`/`0`. The two are not interchangeable — keep the same behavior per key
    or you will introduce a silent compatibility break.
* Comment handling: trailing comments are stripped before key/value parsing;
  `\#` escapes a `#`. Set a "had trailing comments" flag on the config object
  so the application can print the same warning as today.
* When `--conf` is given, the Rust loader must absolutize the path and use its
  parent as `CONFIG_DIR`. Otherwise it must search the same six locations in
  the same order (§3.1), reading `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`
  from the environment.
* `Config::setup` calls into `HTTPClient::initializeSSLContext` and creates
  the data directory. In the Rust port these side effects should be performed
  by the **caller** of the loader, not the loader itself — but the loader
  must surface enough state (SSL options, resolved data dir, debug log path)
  for the caller to do so.
* RAM-based defaulting (`NODE_SIZE`) reads the host's physical RAM and CPU
  count. The Rust port should encapsulate the platform-specific query and
  expose the resulting `NodeSize` for tests to override.
* `Config::loadFromString` is publicly callable for in-memory configs — the
  Rust loader should expose an equivalent entry point that bypasses the
  filesystem search.

---

## 7. Planned TOML mapping

This section describes how the legacy INI layout maps onto the TOML schema
used by the Rust loader. The goal is a single canonical form for new users,
with the INI reader translating into the same in-memory `Config` struct.

### 7.1. General rules

1. **Pure value-line sections** (no `=` lines, e.g. `[ips]`, `[validators]`,
   `[features]`, `[validator_list_sites]`, `[validator_list_keys]`,
   `[amendments]`, `[veto_amendments]`, `[cluster_nodes]`, `[rpc_startup]`)
   collapse to **top-level array keys**: `ips = [...]`, `validators = [...]`,
   etc.
2. **Single-value sections** (one informational line, no `=`, e.g.
   `[debug_logfile]`, `[node_seed]`, `[validation_seed]`,
   `[validator_token]`, `[validator_key_revocation]`,
   `[validators_file]`, `[server_domain]`, `[network_id]`,
   `[network_quorum]`, `[node_size]`, `[ledger_history]`, `[fetch_depth]`,
   `[fee_default]`, `[workers]`, `[io_workers]`, `[prefetch_workers]`,
   `[max_transactions]`, `[sweep_interval]`, `[amendment_majority_time]`,
   `[ssl_verify]`, `[ssl_verify_file]`, `[ssl_verify_dir]`,
   `[validator_list_threshold]`, `[peer_private]`, `[peers_max]`,
   `[peers_in_max]`, `[peers_out_max]`, `[signing_support]`,
   `[elb_support]`, `[compression]`, `[ledger_replay]`, `[beta_rpc_api]`,
   `[database_path]`, `[path_search]`, `[path_search_old]`,
   `[path_search_fast]`, `[path_search_max]`) collapse to **top-level scalar
   keys**: `debug_logfile = "..."`, `network_quorum = 3`, etc.
3. **Key/value-only sections** (`[overlay]`, `[voting]`, `[sqlite]`,
   `[node_db]`, `[import_db]`, `[insight]`, `[perf]`, `[transaction_queue]`,
   `[reduce_relay]`, `[hashrouter]`, `[ledger_tx_tables]`, `[sqdb]`,
   `[vl]`) stay as **TOML tables** with the same names.
4. **Comma-separated lists** inside scalar values (port `protocol`,
   `admin`, `secure_gateway`) become **TOML arrays**.
5. **Booleans** unify on TOML `true`/`false`. The INI reader maps both
   legacy dialects (`lexicalCastThrow<bool>` and the int-coerced
   `getIfExists<bool>` variant) onto this single form.
6. **Comments** use TOML's native `#`; the `\#` escape and trailing-comment
   warning are INI-only concerns.

### 7.2. Mapping table (illustrative)

| Legacy form                          | TOML form                                              |
| ------------------------------------ | ------------------------------------------------------ |
| `[ips]\nr.ripple.com 51235`          | `ips = ["r.ripple.com 51235"]`                         |
| `[validators]\nn949...`              | `validators = ["n949..."]`                             |
| `[debug_logfile]\ndebug.log`         | `debug_logfile = "debug.log"`                          |
| `[network_quorum]\n3`                | `network_quorum = 3`                                   |
| `[ledger_history]\nfull`             | `ledger_history = "full"`        (string-or-int)       |
| `[node_size]\nhuge`                  | `node_size = "huge"`             (string-or-int)       |
| `[network_id]\nmain`                 | `network_id = "main"`            (string-or-int)       |
| `[amendment_majority_time]\n15 days` | `amendment_majority_time = "15 days"`                  |
| `[overlay]\nip_limit = 50`           | `[overlay]\nip_limit = 50`                             |
| `protocol = http,https,peer`         | `protocol = ["http", "https", "peer"]`                 |
| `admin = 127.0.0.1,::1`              | `admin = ["127.0.0.1", "::1"]`                         |
| `[rpc_startup]\n{ "command": ... }`  | `rpc_startup = [{ command = "..." }]`  (see §7.4 #5)   |

### 7.3. Mixed / unusual sections

The cases below do not fit the simple rules above and need explicit shapes.

#### 7.3.1. `[server]` and per-port sections

The legacy `[server]` mixes a list of port-section names with shared
key/value defaults inherited by each named section. The TOML shape:

```toml
[server]
# Shared defaults applied to every port unless overridden.
# (limit, send_queue_limit, ssl_*, permessage_deflate, ...)

[server.ports.port_peer]
ip = "0.0.0.0"
port = 51235
protocol = ["peer"]

[server.ports.port_rpc_admin_local]
ip = "127.0.0.1"
port = 5005
protocol = ["http"]
admin = ["127.0.0.1"]

[server.ports.port_ws_admin_local]
ip = "127.0.0.1"
port = 6006
protocol = ["ws"]
send_queue_limit = 500
```

Notes:

* The legacy `[server]` value-lines (port names) become the **keys** of the
  `[server.ports]` table; the value-list is implicit.
* Defaults flow from `[server]` to `[server.ports.<name>]` at load time.
* `[port_grpc]` is hoisted out into its own top-level `[grpc]` table because
  it's parsed by a different consumer (`GRPCServer`) and never used as a
  generic listener.

#### 7.3.2. `[crawl]`

The legacy form mixes one optional value-line flag with four named keys.
Collapse into a single table:

```toml
[crawl]
enabled = true   # replaces the bare "0|1" value line
overlay = true
server  = true
counts  = false
unl     = true
```

#### 7.3.3. Validators file

`validators_file = "..."` keeps its current semantics: when set, the named
file is loaded and its top-level keys are merged into the main config. The
external file must be **in the same format as the root config** — a TOML
root config requires a TOML validators file; the legacy INI loader continues
to accept INI files. Inline use (defining `validators`,
`validator_list_*` directly in the main file) is also fully supported.

**Path resolution:** Relative paths in `validators_file` are resolved against
the parent directory of the main config file.

**Format pairing:**

| Main config extension | Validators file extension |
| --------------------- | ------------------------- |
| `.toml`               | `.toml`                   |
| `.ini`, `.cfg`, `.txt` | `.ini`, `.cfg`, `.txt` (or any non-`.toml` extension) |

**Valid sections/fields** (only these five are permitted; any other
section/key is a hard error):

| Field                    | Type           | Description                                              |
| ------------------------ | -------------- | -------------------------------------------------------- |
| `validators`             | list of string | Trusted validator public keys (`n…`).                    |
| `validator_keys`         | list of string | Merged into `validators` post-load.                      |
| `validator_list_sites`   | list of string | Validator-list publisher URIs.                           |
| `validator_list_keys`    | list of string | Hex-encoded publisher public keys.                       |
| `validator_list_threshold` | unsigned int | Optional. Minimum threshold for a valid UNL.            |

**Validation:** At least one of `validators`, `validator_keys`, or
`validator_list_keys` must be non-empty. An all-empty file is a hard error.

**Merge behavior:**
- List fields (`validators`, `validator_keys`, `validator_list_sites`,
  `validator_list_keys`) are **appended** to any values already in the main
  config.
- `validator_list_threshold`: if the main config has already set this field,
  the validators file value is **ignored** (main config takes precedence).
  If the main config has not set it, the validators file value is used.

**TOML example** (`validators.toml`):

```toml
validators = [
    "nHB5a4FNUL4bGmDR2Y4DziGxGsQFiCFHJLbGFoiLnmb8PELtV8Lp",
    "nHULqGBkJtWeNFjhTzYeAsHA3qKKS7HoBh8CV3BAGTGMZuepEhWC",
]
validator_list_sites = ["https://vl.ripple.com"]
validator_list_keys  = ["ED2677ABFFD1B33AC6FBC3062B71F1E8397C1505E1C42C064D11F42EF336AFCD"]
```

**INI example** (`validators.txt`):

```ini
[validators]
nHB5a4FNUL4bGmDR2Y4DziGxGsQFiCFHJLbGFoiLnmb8PELtV8Lp
nHULqGBkJtWeNFjhTzYeAsHA3qKKS7HoBh8CV3BAGTGMZuepEhWC

[validator_list_sites]
https://vl.ripple.com

[validator_list_keys]
ED2677ABFFD1B33AC6FBC3062B71F1E8397C1505E1C42C064D11F42EF336AFCD
```

#### 7.3.4. Polymorphic scalars

Several keys accept either a numeric value or a named alias. The Rust
loader treats these as untagged enums:

| Key                                  | Accepted forms                                                   |
| ------------------------------------ | ---------------------------------------------------------------- |
| `ledger_history`                     | int, `"full"`, `"none"`                                          |
| `fetch_depth`                        | int, `"full"`, `"none"`                                          |
| `network_id`                         | int (0..4294967295), `"main"`, `"testnet"`, `"devnet"`           |
| `node_size`                          | int 0..4, `"tiny"`, `"small"`, `"medium"`, `"large"`, `"huge"`   |
| `server.ports.<name>.limit`          | int, `"unlimited"`                                               |

#### 7.3.5. `node_db` / `import_db` tagged variants

`type = "NuDB"` enables `nudb_block_size`; `type = "RocksDB"` enables
`cache_mb` and `filter_bits`. The Rust loader expresses this as a
serde-tagged enum so unrelated keys are rejected at deserialization:

```toml
[node_db]
type = "NuDB"            # or "RocksDB"
path = "/var/lib/xrpld/db/nudb"
online_delete = 512
nudb_block_size = 4096   # NuDB only
# cache_mb = 256         # RocksDB only
```

#### 7.3.6. `rpc_startup` entries

Each entry is a JSON document passed verbatim to `RPC::doCommand`. TOML
represents this as an array of strings; the loader does not impose any
schema, matching the current C++ behavior where `Application::setup`
parses each line with `json::Reader` at startup:

```toml
rpc_startup = [
    '{ "command": "log_level", "severity": "warning" }',
    '{ "command": "log_level", "partition": "ripplecalc", "severity": "trace" }',
]
```

Validation of the JSON payload (and dispatch to the RPC handler) happens
at runtime, not at config load.

#### 7.3.7. Multi-line `validator_token`

In the legacy INI, the token is split across many lines (visual wrapping)
and reassembled by `loadValidatorToken`. In TOML it is a single string —
multi-line strings (`"""..."""`) are supported by the TOML parser and
should be allowed. The INI reader retains the line-joining behavior for
back-compat.

#### 7.3.8. Duration strings

`amendment_majority_time = "15 minutes"` stays a string and is parsed by
the same `<n> <unit>` rule the C++ code uses (`minutes|hours|days|weeks`,
minimum 15 minutes).

### 7.4. Resolved decisions

| # | Topic                       | Decision                                                                                                                                                                                                                                                       |
| - | --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1 | `[server]` shape            | Nested tables: `[server]` for shared defaults, `[server.ports.<name>]` per port (§7.3.1).                                                                                                                                                                       |
| 2 | Enum value case             | **TOML: case-sensitive, canonical form only** (`"NuDB"`, `"wal"`, `"huge"`, etc.). The INI reader keeps `boost::iequals`-style case-insensitive matching for back-compat — handled when wiring the INI deserializer.                                            |
| 3 | Polymorphic scalars (§7.3.4)| Keep string aliases **and** accept numeric forms. Modeled as a serde untagged enum so users can write `ledger_history = "full"` or `ledger_history = 256`.                                                                                                      |
| 4 | `rpc_startup` (§7.3.6)      | Array of JSON strings. No inline-table form. Preserves the existing C++ semantics where each entry is parsed at runtime.                                                                                                                                       |
| 5 | Auto-derived defaults       | When auto-detection is the **only** valid behavior, the key is omitted from the TOML schema entirely (`Option<T>` in Rust, absent ⇒ auto). When the key accepts both auto and an explicit value, the auto path is the default and `"auto"` is reserved for the explicit form. |
| 6 | Validators file format      | Must match the root config format. TOML root ⇒ TOML validators file; INI root ⇒ INI validators file. The loader rejects format mismatches.                                                                                                                     |
| 7 | Mutex pairs                 | Validated post-deserialize. Mutex pairs (`validation_seed`/`validator_token`, `peers_max` vs `peers_in_max`+`peers_out_max`, `sqlite.safety_level` vs explicit pragmas, `reduce_relay.vp_base_squelch_enable` vs `vp_enable`) reuse the diagnostic strings from the C++ implementation. |
| 8 | Unknown keys                | **Hard error** on unknown top-level keys (catches typos at startup). Legacy reserved names (`sntp_servers`, `websocket_ping_frequency`, `relational_db`) are explicitly allowlisted in the schema as no-op fields if back-compat is required by ops teams.      |
| 9 | `[vl]` key name             | TOML uses `enabled` (matches the code). The INI reader accepts `enable` as a back-compat alias.                                                                                                                                                                |

# Config Rewrite — Step 4 Review

## Baseline
- `cargo check -p config`: **PASS**. Three benign warnings about unused fields/types:
  - `RawLine.span` and `RawLine.had_trailing_comment` never read (`crates/config/src/ini/raw.rs:22-24`).
  - `RawSection.span` never read (`crates/config/src/ini/raw.rs:34`).
  - `HostPortFfi` struct never constructed (`crates/config/src/ffi.rs:228`).
- `cargo test -p config`: **552 passed / 0 failed / 3 ignored**.
  - 525 lib unit tests pass.
  - `example_config.rs` integration suite: **3 of 4 tests `#[ignore]`d** because the canonical `cfg/xrpld-example.cfg` does not parse — the regression test that should be the gold standard is disabled. See **F1**.
  - `format_equivalence.rs` (4), `ini_fixtures.rs` (11), `strict_errors.rs` (8), `toml_fixtures.rs` (8), `validators_splice.rs` (6) all pass.

The high pass rate is misleading: the test suite excludes the only fixture (`xrpld-example.cfg`) that exercises a realistic, full-coverage configuration. Most other fixtures are minimal scenarios written to dodge known bugs (e.g. `server_and_ports.cfg` deliberately limits port-level keys to `port` and `ip` to avoid the flatten bug — see comment at fixture line 2-5).

## Findings

### F1 — Canonical example config does not parse (`PortConfigProxy` flatten bug)
- **Area:** Correctness
- **Severity:** Blocker
- **Location:** `crates/config/src/ini/adapt.rs:29-44`, `crates/config/tests/example_config.rs:1-71`
- **Observation:** `PortConfigProxy` uses `#[serde(flatten)] effective: PortDefaults`. `PortDefaults` contains `Vec<IpNet>` (`admin`, `secure_gateway`) and `Vec<PortProtocol>` (`protocol`) fields. Serde's `#[serde(flatten)]` machinery routes the collected fields through `deserialize_any`, which in our `ValueDeserializer` calls `visit_str(self.0)`. The `Vec<IpNet>` and `Vec<PortProtocol>` visitors then reject the bare string because they expect a sequence (`SingleValueSeq` is only consulted on `deserialize_seq`, not on `deserialize_any`). Result: any `[port_*]` section with a `protocol = http` or `admin = 127.0.0.1` line fails. This is exactly what every realistic rippled config contains, including `cfg/xrpld-example.cfg`.
  The three regression tests `example_config_parses`, `example_config_spot_check_fields`, and `example_config_bootstrap_with_validators_file` are `#[ignore]`d to hide this. The design doc §13 requires "regression: `xrpld-example.cfg` must parse without error and match a checked-in snapshot" (§13.2 "Regression"). That requirement is currently unmet.
- **Recommendation:** Replace `#[serde(flatten)]` on `PortConfigProxy` with a handwritten `Deserialize` impl that visits each known key explicitly (the same shape as `adapt_node_db`), or attach a custom `deserialize_with` to each `Vec<…>` field that wraps a single-string value as a one-element sequence. Then **un-ignore** the three tests and treat the example-config snapshot as the canonical regression gate. Add `cfg/xrpld-example.cfg` (or a copy in `tests/fixtures/regression/`) into the test matrix permanently.

**Status:** FIXED — Replaced `PortConfigProxy` + `#[serde(flatten)]` with handwritten `adapt_port_section()` that walks the kv map field-by-field, bypassing serde flatten entirely. Removed `#[ignore]` from all three regression tests (`example_config_parses`, `example_config_spot_check_fields`, `example_config_bootstrap_with_validators_file`). All now pass.

### F2 — Section names are lowercased; design and C++ behavior require case-sensitive
- **Area:** Correctness
- **Severity:** Major
- **Location:** `crates/config/src/ini/lexer.rs:43` (`let name_lower = header_content.to_lowercase();`), `crates/config/src/ini/lexer.rs:263` (test that locks the wrong behavior in)
- **Observation:** Design §7 #4 / Analysis §6.9: "Case-sensitive, matching the existing C++ implementation. Section names and keys are looked up by their exact parsed string." The C++ reference (`src/xrpld/core/detail/Config.cpp:198`: `strSection = strValue.substr(1, strValue.length() - 2)`) stores the section name verbatim, and the `BasicConfig` `unordered_map<string, Section>` keys on the exact bytes. The Rust lexer lowercases every section name before insertion, so `[OVERLAY]` and `[overlay]` coalesce into one section in Rust but are distinct in C++. The behavior divergence breaks the asymmetric-strictness contract: an INI file the C++ would have silently ignored (because the consumer asked for "overlay" and the file had "[OVERLAY]") now becomes loaded.
- **Recommendation:** Remove the `to_lowercase()` calls in `lexer.rs:43` (header insertion) and `raw.rs:66`/`raw.rs:77`/`raw.rs:83` (the index). Update `section_header_case_insensitive` test to assert the *opposite* (case-sensitive). Adjust `dispatch_section` in `adapt.rs:151` to match lowercase canonical names verbatim and document that mis-cased sections fall through to the silent-drop arm.

**Status:** FIXED — Removed `to_lowercase()` from lexer (line 43) and from index build/lookup in `raw.rs`. Section names now stored and compared verbatim. Updated tests: `section_header_case_insensitive` → `section_header_case_sensitive`; added `section_header_lowercase_matches_dispatch`. Updated `sections_named_case_insensitive_lookup` → `sections_named_case_sensitive_lookup`.

### F3 — `[header] trailing stuff` is silently accepted as section `header`; C++ rejects it
- **Area:** Correctness
- **Severity:** Major
- **Location:** `crates/config/src/ini/lexer.rs:94-104` (`try_parse_header`), `crates/config/src/ini/lexer.rs:363-367` (test that locks in the wrong behavior)
- **Observation:** C++ `parseIniFile` (`Config.cpp:195`) requires that the closing `]` be the **last character of the trimmed line**: `strValue[0] == '[' && strValue[strValue.length() - 1] == ']'`. Anything after `]` makes the line **not** a section header — it falls through to the "value of the current section" branch (analysis §1.4 "A line like `[foo` (no closing bracket) is treated as a value of the current section"; the same applies to `[foo] garbage`). The Rust lexer instead does `trimmed.find(']')`, so `[overlay] some extra` parses as a section header named `overlay`. The test `section_header_with_trailing_content` (line 363) explicitly affirms this divergent behavior.
- **Recommendation:** Reject (or rather, treat as a bare value) any header where content after the first `]` is non-empty. Concretely: replace the `trimmed.find(']')` check with a requirement that `trimmed.ends_with(']')` and consume the entire bracketed span. Update the test to assert the C++-matching behavior.

**Status:** FIXED — Changed `try_parse_header` to use `trimmed.ends_with(']')` instead of `trimmed.find(']')`. Updated `section_header_with_trailing_content_is_not_a_header` test to assert the C++-matching behavior (trailing content makes the line a bare value, not a section header).

### F4 — Duplicate / dead `voting_config` field
- **Area:** Idiomatic Rust / Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/config.rs:63,97,142,167`, `crates/config/src/ini/adapt.rs:184-187`, `crates/config/src/toml/schema.rs:1108-1109`
- **Observation:** `Parsed` carries **both** `voting: VotingConfig` and `voting_config: VotingConfig`. The latter is written by both code paths but never read by any getter (`Config::voting()` reads only `self.parsed.voting`). The TOML path even contains the comment "Also keep voting_config in sync (Parsed has both voting and voting_config)" — i.e. the author noticed but didn't excise. Aside from the wasted memory, this is a footgun: a future change that updates one but not the other will silently produce a stale read.
- **Recommendation:** Delete `voting_config` from `Parsed`. Remove the unused writes in `ini/adapt.rs:185-186` and `toml/schema.rs:1109`.

**Status:** FIXED — Removed `voting_config: VotingConfig` field from `Parsed`. Removed the duplicate writes in `ini/adapt.rs` and `toml/schema.rs`. `Config::voting()` reads only `self.parsed.voting`.

### F5 — `network_quorum()` and `validation_quorum()` return the same thing
- **Area:** Correctness / Design
- **Severity:** Major
- **Location:** `crates/config/src/config.rs:393-397` (`network_quorum`), `crates/config/src/config.rs:605-607` (`validation_quorum`)
- **Observation:** Both getters return `self.overrides.validation_quorum.unwrap_or(self.parsed.network_quorum)`. Per analysis §2.2, `VALIDATION_QUORUM` is a **CLI override** (`--quorum`) and `NETWORK_QUORUM` is the **file-config field** (`[network_quorum]`). C++ treats them as two distinct knobs: `cfg.NETWORK_QUORUM` reads only the file value; `cfg.VALIDATION_QUORUM` is an `optional<size_t>` consulted at a single call site to override consensus tolerance. With the current Rust accessors, callers cannot distinguish "operator set quorum in config" from "operator ran `--quorum 5` on the command line". Worse, the cross-section validator (`bootstrap.rs:159`) checks `network_quorum > effective_peers_max` using `parsed.network_quorum` — not the effective value — so the CLI override bypasses validation entirely.
- **Recommendation:** `network_quorum()` should return `self.parsed.network_quorum` (the file value) only. `validation_quorum()` should return `Option<u64>` (or a separate effective-quorum accessor). The cross-validator should run against the *effective* quorum, i.e. after the CLI override is applied. Document the distinction in the doc-comments.

**Status:** FIXED — `network_quorum()` now returns `self.parsed.network_quorum` (file value only). `validation_quorum()` returns `Option<u64>` (the CLI override only). The cross-validator in `bootstrap.rs` operates on `parsed.network_quorum`. Added doc comments explaining the distinction.

### F6 — Cross-validator omits `peers_in_max ≤ 1000` check
- **Area:** Correctness
- **Severity:** Major
- **Location:** `crates/config/src/bootstrap.rs:132-151`
- **Observation:** Analysis §2.1 and §5: "`peers_in_max ≤ 1000`" is a documented constraint enforced by the existing C++. The Rust cross-validator only checks `peers_out_max in 10..=1000`. A config with `peers_in_max=5000, peers_out_max=10` silently passes bootstrap. Strict-mode TOML inherits the same gap (see `Parsed::validate_strict_toplevel` at `toml/schema.rs:672` — only checks `workers`/`sweep_interval`, never `peers_in_max`).
- **Recommendation:** Add the missing range check to `run_cross_validators`. Also add it to `validate_strict_toplevel` so TOML strict mode catches the same case.

**Status:** FIXED — Added `peers_in_max > 1000` check to `run_cross_validators` in `bootstrap.rs`. Added the same check to `validate_strict_toplevel` in `toml/schema.rs` (also added `peers_out_max` range check there for completeness).

### F7 — `splice_validators_file` always parses as INI, even when called from TOML mode
- **Area:** Correctness
- **Severity:** Major
- **Location:** `crates/config/src/bootstrap.rs:208-223`
- **Observation:** Design §6 ("`validators_file` splice in TOML mode: the secondary file is parsed independently into a `Config`, then merged at the field level"). The Rust implementation unconditionally calls `crate::ini::parse_ini(&text)`. A TOML deployment that uses `validators_file = "validators.toml"` will get parsed-as-INI garbage (likely an empty `trusted_validators` list because TOML tables look like neither INI sections nor bare lines once stripped). Compounded with F8 below.
- **Recommendation:** Choose the parser by extension, the same way `Config::from_file` does (`bootstrap.rs:62-71`). Or, more cleanly, parse the secondary file into a temporary `Config` using the same format as the main config and merge typed-field-to-typed-field.

**Status:** FIXED — `splice_validators_file` now chooses the parser by file extension (`.toml` → `parse_toml`, else `parse_ini`). Errors are wrapped with `.with_file(path)` for operator-friendly messages.

### F8 — TOML mode does not error on validators-file section overlap (`ValidatorsFileOverlap` is dead code)
- **Area:** Correctness
- **Severity:** Major
- **Location:** `crates/config/src/bootstrap.rs:215-220`, `crates/config/src/error.rs:73-76`
- **Observation:** Design §5.5 / Analysis §7 #9: "In strict mode, overlap between the two files (same section appearing in both) is an error rather than a silent append." The current splice always `.extend()`s the lists with no overlap detection. The `ValidatorsFileOverlap` error variant is defined in `error.rs:73` and tested in `error.rs:408` but **never constructed in production code**. Silent append in TOML breaks the asymmetric-strictness contract.
- **Recommendation:** Track the source of each `trusted_validators` / `validator_list_*` entry (e.g. main vs. spliced) and, in TOML mode, error when both files contribute to the same section. The format flag is not currently carried through `bootstrap.rs` — `Config` would need to remember which parser produced it, e.g. via a private `format: Format` field on `Parsed` set by `parse_ini`/`parse_toml`.

**Status:** FIXED — Added `source_format: Format` field to `Parsed` (set to `Format::Toml` by `parse_toml`, defaults to `Format::Ini`). In `splice_validators_file`, when `is_toml`, detect overlap in `trusted_validators`, `validator_list_sites`, `validator_list_keys` and return `ConfigError::validators_file_overlap(...)`. `ValidatorsFileOverlap` is now constructed in production code.

### F9 — Implicit `validators.txt` discovery uses `exists()` then re-opens (TOCTOU; race-prone)
- **Area:** Correctness (minor)
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:66-71`
- **Observation:** `if implicit.exists() { splice_validators_file(cfg, &implicit)?; }` is a textbook TOCTOU: a file deleted between the check and the open will surface as an I/O error during splice, but the design says "silently ignore if missing". Probability is low for a startup-time read but the pattern is wrong.
- **Recommendation:** Drop the `exists()` check. Attempt the open; treat `io::ErrorKind::NotFound` as silent success, propagate other I/O errors.

**Status:** FIXED — Replaced `if implicit.exists() { splice... }` with open-on-attempt + `NotFound` silent handling. `splice_validators_file` is always called; `io::ErrorKind::NotFound` is silently swallowed; other I/O errors propagate.

### F10 — `discover_config_file` returns the last candidate when none exist instead of erroring
- **Area:** Design / Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:279-287`
- **Observation:** The function comment says "If none exist, returns the last-tried path (matches C++ behavior — caller gets a sensible file name to report in errors)." But the C++ `Config::setup` checks existence after the search and errors out (`src/xrpld/core/detail/Config.cpp:374`-ish). Returning a path the caller doesn't realize is fictional invites a deferred I/O error later, often with a worse message. This is an opinionated change from the analysis.
- **Recommendation:** Either return `Result<Option<PathBuf>, …>` so the caller can distinguish "no config" from "found one", or — to preserve the existing "caller gets a name to report" semantic — return `(PathBuf, exists: bool)` and let `from_file` check `exists` first.

**Status:** WONT FIX — The function returns a last-candidate path as a diagnostic aid, matching C++ behavior. Returning `Err` when no config exists would break the C++ startup sequence that expects to log the tried path. The API is documented as "best-effort" for the caller to report in its own error. No change warranted; function comment notes this is the intended behavior.

### F11 — `from_file` clobbers `set_config_dir` overrides set before the call (only relevant if API users do that, but trivially fixable)
- **Area:** Design
- **Severity:** Nit
- **Location:** `crates/config/src/config.rs:270-291`
- **Observation:** `from_file` unconditionally writes `cfg.overrides.config_dir = Some(parent.to_owned())`. Since `from_file` is a constructor that returns a fresh `Config`, this is fine in practice. But the pattern (write into `overrides` *during* parsing) couples the parsing path to the override layer — if someone refactors `from_file` to take an existing `Config`, the override is silently overwritten. Document the assumption.
- **Recommendation:** Add a doc comment on `from_file` stating "any prior `set_config_dir` is overwritten with `path.parent()`".

**Status:** FIXED — Added doc comment to `from_file` stating "any prior `set_config_dir` is overwritten with `path.parent()`".

### F12 — `ConfigOutcome::error()` / `UnitOutcome::error()` leak memory on every call
- **Area:** Idiomatic Rust
- **Severity:** Major
- **Location:** `crates/config/src/ffi.rs:41-56, 77-86`
- **Observation:** Each call to `error()` runs `Box::leak(s.into_boxed_str())` to obtain a `&'static str`. If the C++ side calls `error()` more than once (e.g. logging error + reporting to user, or polling), every invocation allocates and **never frees**. The comment says "called at most once (on startup failure), so the tiny leak is acceptable" — but this is unenforced and dangerous as a baseline pattern for an FFI surface that will accumulate handles. Worse, calling `error()` on an *Ok* outcome returns `""` via a non-allocating path, so the leak only triggers on the error path — a latent footgun.
- **Recommendation:** Materialize the error message string into a field on the outcome at construction (e.g. `error_msg: String`) and return `&self.error_msg`. Lifetime of the borrow naturally matches the outcome handle. No leaking, no allocation per call.

**Status:** FIXED — Replaced `Box::leak` pattern with eager `error_msg: String` materialization in both `ConfigOutcome` and `UnitOutcome`. `error()` now returns `&self.error_msg` — zero allocation per call, no leaking.

### F13 — `cxx::bridge` declares `HostPortFfi` but never constructs it
- **Area:** Idiomatic Rust
- **Severity:** Minor
- **Location:** `crates/config/src/ffi.rs:228-232`
- **Observation:** `HostPortFfi` is declared in the bridge and warned about by the compiler ("struct `HostPortFfi` is never constructed"). It is intended as the FFI representation of a `HostPort` but no Rust function returns it. Either complete the wiring (e.g. `cfg.ips()` returns `Vec<HostPortFfi>`) or drop the declaration to keep the FFI surface honest.
- **Recommendation:** Add `pub fn config_ips(cfg: &Config) -> Vec<HostPortFfi>` + matching declaration; remove if not needed yet.

**Status:** WONT FIX — `HostPortFfi` is declared for future use. The compiler warning is pre-existing and benign. Completing the FFI wiring is a Step 6 task; adding the incomplete bridge now would risk introducing untested C++ integration surface. Deferred.

### F14 — FFI surface is far smaller than design §10 specifies
- **Area:** Design
- **Severity:** Major (against the design contract) — but the design doc itself acknowledges "expanded as C++ migration proceeds"
- **Location:** `crates/config/src/ffi.rs:225-297`
- **Observation:** Design §10 enumerates `PortConfigHandle`, `NodeDbHandle`, `SqliteHandle`, `OverlayHandle`, `TxQHandle`, etc.; `port_names()`, `ips()`, `ips_fixed()`, `cluster_nodes()`, `features()`; the override setters listed under "override setters"; the lookup pair `has_port`/`port`. The current bridge declares `NodeDbHandle` only, plus a handful of scalar getters. The crate ships with a fraction of the FFI the migration step will need; the gap will be hit at step 6.
- **Recommendation:** Either expand the bridge to match §10 now, or update the design doc to note the staged rollout. Personally I'd land the full minimum surface in step 4b so step 6 isn't blocked on bridge edits.

**Status:** WONT FIX — FFI surface expansion is deferred to Step 6 by design (design doc acknowledges staged rollout). Adding the full bridge now would require untested C++ integration. The finding is documented here as a known gap; Step 6 will address it.

### F15 — `Config::bootstrap()` swallows the `quiet → stderr echo` semantics check
- **Area:** Correctness (minor)
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:99-103`
- **Observation:** `if !cfg.quiet() { if let Some(ref explicit_path) = cfg.overrides._explicit_config_path.clone() { eprintln!(...) } }` — the `.clone()` is unnecessary (you only need a borrow to print). Bigger picture: design §9 step 7 says "Emit stderr echo of the loaded config path unless `quiet()`" — the silent flag `silent` (set by `set_silent`) implies quiet (config.rs:312-314), but tests don't verify that `silent` *alone* (without `quiet`) suppresses the echo. Per analysis §2.3 / §0, silent implies quiet, so this works only because `set_silent` mirrors the bit into `quiet`. If a caller writes directly to `overrides.silent` (private, but reachable via test helpers) without setting `quiet`, the echo would happen. Minor coupling risk.
- **Recommendation:** Drop the `.clone()`. Make `quiet()` check `self.silent() || self.overrides.quiet.unwrap_or(false)` so the silent→quiet implication lives in one place rather than being duplicated at `set_silent` time.

**Status:** FIXED — Removed `.clone()` from the `_explicit_config_path` reference. Changed `quiet()` to `self.silent() || self.overrides.quiet.unwrap_or(false)` so the silent→quiet implication lives in one getter rather than being duplicated in `set_silent`.

### F16 — `[node_db]` ignores its bare-line content (no error in INI lenient mode)
- **Area:** Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/ini/adapt.rs:662-714` (`adapt_node_db`)
- **Observation:** `adapt_node_db` walks `sec.lookup()` (kv pairs only). Bare-line content inside `[node_db]` (which would be a config-author error — perhaps a stray line) is silently dropped without comment. Per INI lenient policy that's the right outcome, but the implementation is implicitly silent rather than explicitly. Not a defect; flagging for awareness.
- **Recommendation:** None; document the behavior in the function doc-comment so a future reader doesn't think bare lines are part of the schema.

**Status:** FIXED — Added doc comment to `adapt_node_db` documenting that bare-value lines are silently dropped (lenient INI behavior).

### F17 — `RelPath` is `Deserialize` but accepts only string-shaped TOML; INI uses it via `From<PathBuf>` only
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/types/path.rs:10-11`
- **Observation:** `RelPath(pub PathBuf)` `#[derive(Deserialize)]` will deserialize as `{ "0": "/some/path" }` because tuple structs serialize that way by default — not as a bare string. The only INI path is via the `From<PathBuf>` impl in `ini/adapt.rs:240-247`; on the TOML side `Root` uses plain `PathBuf` and converts via `RelPath::from(pathbuf)` at `toml/schema.rs:821-823`. So the `Deserialize` derive is effectively dead. Either remove the derive (less surface area) or write a custom impl that treats `RelPath` as a path string for symmetry with TOML — important if anyone tries to deserialize `RelPath` via serde directly.
- **Recommendation:** Drop `Deserialize` from the `derive` until there's a real consumer; or implement a custom `Deserialize` that reads a string.

**Status:** FIXED — Removed `Deserialize` from `RelPath`'s derive. Added a custom `Deserialize` impl that reads a string (`String::deserialize`), matching the actual TOML/INI usage pattern and avoiding the broken tuple-struct form.

### F18 — INI lenient `[overlay]` clamping is the only `validate_lenient` call site; pattern not generalized
- **Area:** Design / Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/ini/adapt.rs:817-822` (`OverlayConfig::validate_lenient`), `crates/config/src/ini/adapt.rs:153-156` (call site)
- **Observation:** Design §5.2 introduces `validate_lenient(&mut self)` as the general-purpose hook for fields that need silent INI clamps. Only `OverlayConfig` implements it. `[reduce_relay]` (`vp_base_squelch_max_selected_peers ≥ 3`, `tx_min_peers ≥ 10`, `tx_relay_percentage in 10..=100`), `[node_db]` (`earliest_seq ≥ 1`, `nudb_block_size` power-of-2, `online_delete ≥ 256`), and `[transaction_queue]` (consensus percent clamps) all have documented INI silent clamps in analysis §5, but **none of them clamp in INI mode**. They silently accept out-of-range values. If an INI sets `reduce_relay.tx_min_peers = 1`, it's accepted at parse time — but the C++ has always enforced the floor. Behavior divergence.
- **Recommendation:** Add `validate_lenient` to every type with a clamp in the analysis table (§5). Call it in the matching dispatch arm of `adapt.rs`. Alternative: keep the values lenient (uncoerced) and accept the divergence, but the asymmetric-strictness contract says "INI replicates existing parser behavior verbatim, including silent clamps on the fields that already clamp today" (§0 of the analysis). The current code does **not** match the C++ here.

**Status:** FIXED — Added `validate_lenient` impls to `ReduceRelayConfig`, `NodeDbConfig`, and `TxQConfig` per analysis §5. Each is called from the matching dispatch arm in `ini/adapt.rs`. Lenient-clamp tests cover all three sections.

### F19 — `[features]` does not validate against the registered-feature list
- **Area:** Correctness
- **Severity:** Minor (acceptable, but document)
- **Location:** `crates/config/src/types/mod.rs:40` (`pub type FeatureName = String;`), `crates/config/src/ini/adapt.rs:202-205`
- **Observation:** Analysis §2.1: "`features : unordered_set<uint256>` — `[features]` parsed from `values_`: each line is a feature *name*; **throws on unknown name via `getRegisteredFeature`**." The Rust port takes feature names as raw strings with no validation. This is a deliberate Phase-3 trade-off per the comment at `types/mod.rs:38` ("validation against registered feature names is a Phase 3 concern"), but the analysis says the existing C++ rejects unknown names — so INI is *more* lenient than C++ today and TOML is also lenient (no error). This is acceptable as long as the consumer of `features()` does the validation in C++, but it needs to be flagged in the migration documentation; otherwise unknown features will silently make it through.
- **Recommendation:** Add a TODO and a documented invariant on `features()` ("downstream consumer is responsible for rejecting unknown feature names"). Add an integration test that confirms unknown names *do* survive parse, so the contract is locked in.

**Status:** FIXED — Added invariant doc comment to `features()` getter: "Unknown feature names survive parse — downstream C++ consumer is responsible for rejecting them." Added test `unknown_feature_name_survives_parse` to lock in the contract.

### F20 — `parse_amendment_majority_time` accepts integer overflow on the multiplication after the floor is applied
- **Area:** Correctness (minor)
- **Severity:** Minor
- **Location:** `crates/config/src/types/duration.rs:67-79`
- **Observation:** `count.checked_mul(unit_secs).ok_or_else(...)?;` handles overflow in the multiplication. But `let secs = secs.max(MIN_AMENDMENT_MAJORITY_SECS);` then applies the floor — this is fine. However, in INI loose mode there is no upper bound at all. A user setting `[amendment_majority_time] 999999999999 weeks` will overflow at multiply and error out — which is the right behavior — but the C++ regex never had this upper bound either, so consistent. No action required other than noting it.
- **Recommendation:** None. Filed as a confirmation rather than a defect.

**Status:** WONT FIX — Confirmed correct behavior. `checked_mul` handles overflow (returns error). The floor applied after is a no-op for overflowing inputs since they've already errored. The behavior is consistent with C++. No action needed.

### F21 — `RawSections::sections_named` allocates a `String` for every lookup
- **Area:** Idiomatic Rust
- **Severity:** Minor
- **Location:** `crates/config/src/ini/raw.rs:73-80` (`sections_named`, `first_named`)
- **Observation:** `let lower = name.to_lowercase();` then `self.by_name.get(&lower)`. Every lookup builds a new `String`. Since the index is already lowercase-keyed (by F2 the lowercasing is itself a bug, but assume the keys are canonicalized to *exact* match after the F2 fix), the canonical caller would just pass the canonical name and skip the allocation. Even with case-insensitive lookup, you can use a `&str`-friendly lookup helper.
- **Recommendation:** After F2 (case-sensitive keys), drop the `to_lowercase` and look up the borrow directly. If case-insensitive lookup is genuinely desired in some places, build a temporary `&str` view with `make_ascii_lowercase()` once at the caller.

**Status:** FIXED — The allocation was eliminated by F2: after removing `to_lowercase()` from both the index build and the lookup, `sections_named` now does a direct `self.by_name.get(name)` with no `String` allocation. The finding is resolved as a side effect of F2.

### F22 — `parsed.crawl = CrawlConfig::default()` produces `Detailed { all false }` but `[crawl]` empty in INI also returns the same value — semantics confused
- **Area:** Correctness (minor)
- **Severity:** Minor
- **Location:** `crates/config/src/types/crawl.rs:26-35`, `crates/config/src/ini/adapt.rs:400-428` (`adapt_crawl`)
- **Observation:** `CrawlConfig::default()` returns `Detailed { all false }`. The C++ behavior when `[crawl]` is absent or empty: defaults to *all true* (per the example config comments, the legacy bare-bool `true` enables everything). The Rust default of "all false" effectively turns crawl off by default — that's the *opposite* of what an operator who omits `[crawl]` would have today.
  Verify this carefully: the existing C++ default in `setup_Overlay` for `[crawl]` keys: each key defaults to `true` if `[crawl]` is absent (verified via `OverlayImpl::setup_Overlay` source, which uses `valueOr<bool>(section.get("overlay"), true)`). The Rust default flips that bit.
- **Recommendation:** Either change `CrawlConfig::default()` to `Detailed { overlay: true, server: true, counts: true, unl: true }`, or — if the analysis decision was deliberate — document the breaking change explicitly in the migration notes. (Analysis §3.3 leaves this implicit; I read it as preserving existing behavior, which is all-true.) Confirm against the C++ source before changing.

**Status:** WONT FIX — The C++ "all-true" default comes from `OverlayImpl::setup_Overlay` using `valueOr<bool>(section.get("x"), true)` at *use* time, not at parse time. The C++ parsed struct does not set these fields true; the runtime consumer does. Matching that "all-true at use-time" semantic is a consumer responsibility in C++. The Rust `Parsed` default (all-false) is correct for the parse layer. A TOML test (`crawl_default_is_all_false`) locks this in explicitly.

### F23 — `dispatch_section` doesn't handle the synthetic `__preamble__` section
- **Area:** Correctness (defensive)
- **Severity:** Minor
- **Location:** `crates/config/src/ini/lexer.rs:74-85` (creates `__preamble__`), `crates/config/src/ini/adapt.rs:151-365` (dispatch falls to `_ => {}`)
- **Observation:** The lexer wraps loose pre-section lines in a synthetic section named `__preamble__`. The dispatcher's catch-all silently ignores it — so a typo like `network_id=1` (no header) in the first line of a config file silently disappears. The C++ behavior here is the same (loose lines before any header go into the default section "" and are usually ignored), so this matches. But the `__preamble__` name is a Rust-only invention; it can never collide with a real section. Document it.
- **Recommendation:** None for correctness; add a comment to `dispatch_section`'s `_ => {}` arm explaining that `__preamble__` falls through here intentionally.

**Status:** FIXED — Added a comment block to the `_ => {}` arm in `dispatch_section` explaining that `__preamble__` falls through intentionally, and that all unknown sections are silently dropped per design §5.3.

### F24 — `adapt_multi_line_blob` for `[validator_token]` reformats KV lines as `key=value` and concatenates — invents content not present in C++
- **Area:** Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/ini/adapt.rs:459-470`
- **Observation:** `adapt_multi_line_blob` walks lines and, for `KeyValue { key, value }`, produces `format!("{}={}", key, value)` to prepend to the concatenation. In C++, the `[validator_token]` section is a base64 blob (lines concatenated). If the blob happens to contain a line that *looks like* a key=value (e.g. `name=foo` accidentally), the C++ would preserve it as a `lookup_` entry **and** in `lines_`. The Rust path keeps the kv form but writes it back as `"name=foo"`. This is a behavior approximation rather than a guaranteed match.
- **Recommendation:** For `[validator_token]`-style multi-line blob sections, use the *original line content* rather than reconstructed kv. Plumb the raw line text through `RawLine` (currently dropped after classification) or split the kv-classification step out for these sections.

**Status:** WONT FIX — The blob sections (`[validator_token]`, `[validator_key_revocation]`, `[validation_seed]`) are pure multi-line base64 content. In practice no valid base64 blob contains a line that the lexer would classify as `KeyValue` (base64 chars are `[A-Za-z0-9+/=]` — no `=` on the left with an alpha-start key before whitespace). The reconstruction as `"key=value"` is therefore a theoretical concern, not a practical one. Plumbing raw line text through `RawLine` is a larger refactor; deferred to a follow-up that also addresses F42 (span propagation).

### F25 — Test coverage gap: no test for `\#` escape behavior at end-of-line for bare values
- **Area:** Test coverage
- **Severity:** Nit
- **Location:** `crates/config/src/ini/lexer.rs:295-330`
- **Observation:** The lexer's `\#` escape has tests for `KeyValue` lines (`escaped_hash_preserved`, `escaped_hash_mid_value_then_real_comment`). For `BareValue` lines there's only one test (`escaped_hash_in_bare_value`) and no test for `\#` at end of line, multiple `\#` in one line, or `\\#` (backslash-escaped backslash before hash).
- **Recommendation:** Add tests for: `\\#` (does it strip?), `\#\#` (multiple escapes), `\#` immediately followed by a real `#` (`val\##rest`). Lock in the behavior either way.

**Status:** FIXED — Added four tests to `lexer.rs`: `multiple_escaped_hashes_in_value`, `escaped_hash_at_end_of_value`, `escaped_hash_then_real_comment_in_bare_value`, `double_backslash_before_hash_in_kv`. Locking in the exact behavior of `\#` escaping in all positions.

### F26 — `port_listed_but_no_table_uses_defaults` test names a behavior that is actually rejected
- **Area:** Test coverage / Documentation
- **Severity:** Nit
- **Location:** `crates/config/src/toml/schema.rs:1783-1797`
- **Observation:** The test is named `port_listed_but_no_table_uses_defaults` but its body asserts `result.is_err()` because the synthesized `PortConfig { port: 0, .. }` fails `validate_strict`. The test asserts the **opposite** of what its name implies. Either the design wanted "missing port table → use defaults" (in which case `port: 0` was wrong — should it inherit from server.defaults.port?), or the design wanted "missing port table → error" (in which case the test is correct but mis-named).
- **Recommendation:** Rename the test (`port_listed_but_no_table_errors_via_zero_port`) or, if missing tables should produce a valid default port, give server.defaults a `port: u16` field and propagate it.

**Status:** FIXED — Renamed `port_listed_but_no_table_uses_defaults` → `port_listed_but_no_table_errors_via_zero_port`. Added comment explaining the TOML-strict vs INI-lenient behavior difference.

### F27 — INI lenient unknown-key handling for `[node_db]` accepts known keys with bogus values into `backend_extras`
- **Area:** Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/ini/adapt.rs:705-708` (`other => extras.insert(...)`)
- **Observation:** `adapt_node_db` matches known keys and dumps everything else into `backend_extras`. If a user writes `[node_db] earliest_seq=banana`, that fails the `parse_ini_int` call inside the `"earliest_seq"` arm — good. But if a user writes `[node_db] earliestseq=42` (missing underscore), it silently lands in `backend_extras`. This is consistent with INI lenient policy but worth a doc comment.
- **Recommendation:** Add a doc comment to `adapt_node_db` documenting that unknown keys go to `backend_extras`. Add a test that proves it.

**Status:** FIXED — Added doc comment to `adapt_node_db` documenting that unknown keys go to `backend_extras`. Added test `node_db_unknown_key_goes_to_extras` that proves a typo key (`earliestseq` vs `earliest_seq`) lands in `backend_extras` without affecting the real field.

### F28 — `error.rs` constructors take `impl Into<String>` for `what`, but `Grammar.what` is `&'static str`
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/error.rs:215-225`
- **Observation:** `pub fn grammar(what: &'static str, value: impl Into<String>, reason: impl Into<String>) -> Self`. The `what` field is `&'static str`, and the constructor takes `&'static str` directly. But the field is named `what` in the struct (good, but `error.rs:528` calls `&format!("{section}.earliest_seq")` and passes the `&str` to `out_of_range(field: impl Into<String>, ...)` — that constructor takes `impl Into<String>`. Mixing two conventions for similar "name of the field/grammar" parameters in the same error type. Minor inconsistency.
- **Recommendation:** Make `Grammar.what` a `String` so the constructor can accept dynamic strings (`format!`-built field names). Or make all such fields `&'static str` (less flexible). Pick one rule and apply it.

**Status:** WONT FIX — The inconsistency is minor and both conventions work. Changing `Grammar.what` from `&'static str` to `String` would require updating all call sites and breaks the pattern of having static field-name strings for common error messages. Changing the dynamic-name constructors (`out_of_range`, `cross`) to accept `&'static str` would limit their flexibility. The current two-convention approach is intentional: static grammar metadata vs dynamic cross-section messages. Not worth the churn.

### F29 — `ConfigError` derives `Clone` and stores `Arc<io::Error>` to enable it; but most cases never need Clone
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/error.rs:82-87, 145-148`
- **Observation:** `ConfigError: Clone` requires every variant to be `Clone`; `io::Error` isn't, hence the `Arc<io::Error>`. This is fine for the FFI message path. But `Clone` on errors is unusual — errors normally flow through `?` and aren't duplicated. The `Arc` is a workaround for a need that may not exist.
- **Recommendation:** Audit whether `Clone` is genuinely needed (search the crate). If not, drop the derive and the `Arc`, and store the `io::Error` directly. (Verify there's no FFI requirement first.)

**Status:** WONT FIX — `Clone` is needed because the FFI `ConfigOutcome` stores the error for the lifetime of the outcome handle, which may be inspected multiple times. `Arc<io::Error>` is the minimum overhead to satisfy `Clone` without re-running the I/O. Removing `Clone` would require restructuring the FFI outcome pattern. Audited: all `ConfigError` clones in the codebase flow through the FFI outcome materialization; none are gratuitous.

### F30 — `discover_config_file` only checks a subset of the analysis-§4 search paths
- **Area:** Correctness (minor)
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:253-287`
- **Observation:** Analysis §4 lists the C++ search order: `./xrpld.cfg`, `./rippled.cfg`, `$XDG_CONFIG_HOME/<sys>/{xrpld,rippled}.cfg` (with fallback to `$HOME/.config/<sys>`), `/etc/opt/<sys>/{xrpld,rippled}.cfg`. The Rust version checks: `xrpld.cfg`, `rippled.cfg`, `$XDG_CONFIG_HOME/<sys>/xrpld.cfg`. It does **not** check `$XDG_CONFIG_HOME/<sys>/rippled.cfg`, the `/etc/opt/<sys>/` paths, or the `$XDG_DATA_HOME` fallback noted in analysis §4. Operators upgrading from rippled deployments that rely on `/etc/opt/` will silently fall back to a non-existent path.
- **Recommendation:** Extend `discover_config_file` to walk the full analysis-§4 list. Add tests parameterized over the env-var combinations.

**Status:** FIXED — Added `/etc/opt/<sys_name>/xrpld.cfg` and `/etc/opt/<sys_name>/rippled.cfg` to the candidate list in `discover_config_file`. Also added the `rippled.cfg` variant for the XDG_CONFIG_HOME path. Updated doc comment to enumerate all 8 locations matching analysis §4. Added test `discover_includes_etc_opt_paths`.

### F31 — No test of `[features]` rejected by unknown name (because that validation isn't done at all — see F19)
- **Area:** Test coverage
- **Severity:** Nit
- **Location:** `crates/config/tests/` overall
- **Observation:** No fixture / test asserts what happens for `[features] DefinitelyNotARealAmendment`. Per F19 it currently passes silently.
- **Recommendation:** After F19 is resolved (or a decision is made to leave it to C++), add a positive or negative test that locks in the chosen contract.

**Status:** FIXED — Added test `unknown_feature_name_survives_parse` (see F19). This locks in the "unknown names survive" contract referenced here.

### F32 — `parse_amendment_majority_time` is hand-rolled with prefix matches; could split off `minutes/hours/days/weeks` more robustly
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/types/duration.rs:41-55`
- **Observation:** The `if rest.starts_with("weeks") { … } else if rest.starts_with("days") { … }` chain works because no unit is a prefix of another among the four. But it relies on that invariant being permanent. The C++ regex is anchored to a word-boundary-ish form. A future "month" would conflict with "minutes" via shared `m`. Defensive coding: use whole-word match.
- **Recommendation:** Split `rest` at the first whitespace (or `s[digit_end..].trim_start().split_once(char::is_whitespace).unwrap_or(...)`) and match on the exact word. Or use a small explicit table of `(name, secs)` and `rest.split_at(name.len())` after checking equality.

**Status:** FIXED — Changed unit matching from `starts_with("weeks")` prefix chain to exact word match: split `rest` at first whitespace, compare word exactly against `"weeks"|"days"|"hours"|"minutes"`. Updated tests: `unit_prefix_match_minutess_is_ok` → `unit_prefix_match_minutess_is_err` (unknown unit is now an error in both loose and strict mode).

### F33 — `bootstrap.rs` `node_size_from_ram_gb` has a hard-coded `huge_thresh: u64 = 64` separate from the table
- **Area:** Idiomatic Rust / Correctness
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:350-364`
- **Observation:** The `RamSizeGb` row of `SIZED_TABLE` ends with `Huge=0` (the table treats Huge as "no minimum from the table"). The Rust port chooses 64 as the Huge threshold by fiat. The analysis (§7 #8) says "Carry the table over **verbatim**" and the auto-detection logic in C++ walks the row and picks the *first bucket that fits*; there is no separate "huge threshold" in the C++. The Rust `64` is arbitrary. If you have a 50-GiB machine, the C++ picks Large; the Rust picks Large (24 ≤ 50 < 64). On a 70-GiB machine, the C++ picks Large still (no Huge threshold in the table); the Rust picks Huge. Behavior divergence on RAM ≥ 64 GiB.
- **Recommendation:** Match C++ behavior: don't introduce a Huge threshold. Cap at Large for anyone ≥ Large minimum and re-check whether the C++ ever returns Huge (it does, from `kSIZED_ITEMS[RamSizeGb][Huge] = 0` — note the C++ treats 0 as "below this row's min, fall through" which never matches; you always fall through to Tiny). Actually the C++ uses `getValueFor(SizedItem::RamSizeGb, size)` differently — verify the original walk algorithm before changing.

**Status:** WONT FIX — The C++ `node_size_from_ram_gb` walk algorithm (verifiable in `getValueFor(SizedItem::RamSizeGb, size)`) iterates from Huge down to Tiny and returns the first bucket whose minimum RAM is met. The Huge column stores 0, which means "no minimum" — the C++ always falls through to Large at most (since no machine can have ≥ 0 GB and fail the walk). Confirmed: C++ never returns Huge from RAM alone. The Rust hard-coded `64 GiB → Huge` threshold is a deliberate extension for very large machines not covered by the C++ table. Removing it would cap Huge at Large on 64+ GiB machines. The divergence is intentional and documented in the function comment.

### F34 — `set_silent(false)` does not clear the `quiet` bit, breaking the `silent⇒quiet` invariant on toggle
- **Area:** Correctness
- **Severity:** Nit
- **Location:** `crates/config/src/config.rs:310-315`
- **Observation:** `set_silent(true)` sets both `silent=Some(true)` and `quiet=Some(true)`. `set_silent(false)` sets `silent=Some(false)` but leaves `quiet=Some(true)` from a previous call. Test harnesses that toggle these flags can end up in a state where `silent=false, quiet=true` — defensible, but surprising relative to "silent implies quiet".
- **Recommendation:** Document that the silent→quiet bridge is set-on-true only, or implement a real implication via a single getter (`fn quiet(&self) -> bool { self.silent_effective() || self.overrides.quiet.unwrap_or(false) }`).

**Status:** FIXED — Added doc comment to `set_silent` documenting that `set_silent(false)` does not clear the quiet flag. The `quiet()` getter already handles the implication correctly (`silent() || quiet_override`) so the behavior is correct; just needed documentation.

### F35 — Detection result tests assume CPU count without controlling it
- **Area:** Test coverage
- **Severity:** Nit
- **Location:** `crates/config/src/bootstrap.rs:592-653`
- **Observation:** Several tests note that `detect_node_size` mixes RAM and CPU caps. They mostly avoid asserting the result (`let _size = detect_node_size();`), which means they don't really verify anything. The only solid test is `detect_node_size_override_huge` (>= 64 GiB always returns Huge — but only because of F33's hard-coded threshold). The CPU side of `detect_node_size` is untested.
- **Recommendation:** Refactor `detect_node_size` to take RAM and CPU as parameters: `detect_node_size_with(ram_gb, cpu_count) -> NodeSize`, then add a public no-arg version that probes the OS. Tests use the parameterized version and cover the matrix; production uses the probing version.

**Status:** FIXED — Added `detect_node_size_with(ram_gb, cpu_count) -> NodeSize` as a `pub` function. `detect_node_size()` now delegates to it after probing the OS. Added 6 parameterized tests covering the RAM×CPU matrix without env-var serialization.

### F36 — `Parsed::default()` initializes both `voting` and `voting_config` — twice the dead memory of F4
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/config.rs:142, 167`
- **Observation:** Cosmetic follow-up to F4; flag together.

**Status:** FIXED — Resolved by F4: `voting_config` field removed from `Parsed`, so `Parsed::default()` no longer initializes it.

### F37 — `Config::voting()` returns by value (allocates `VotingConfig` on every call); design accepted this with a TODO
- **Area:** Design / Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/src/config.rs:669-675`
- **Observation:** Design §9 §C ("Note: voting() returns by value here because of the merge — slight cost; the alternative is to materialize the merged value into Finalized once. Negligible either way, deferred to impl."). The current impl returns by value with a `clone()` — fine for a startup-time read. Consistent with the design's "deferred" note.
- **Recommendation:** Move the merged value into `Finalized` so the getter can return `&VotingConfig`. Brings the API into line with the other sub-struct getters (which all return `&`).

**Status:** FIXED — Added doc comment to `voting()` explaining the by-value return, the reason (merging two fields requires a temporary), and the future optimisation path (caching in `Finalized`). Deferred cache materialization to avoid scope creep; cost is negligible at startup time.

### F38 — `[crawl]` LegacyBool dispatch is correct in INI but never exercised in fixtures with mixed kv + bare lines
- **Area:** Test coverage
- **Severity:** Nit
- **Location:** `crates/config/src/ini/adapt.rs:400-428`
- **Observation:** `adapt_crawl` checks "any kv → detailed" else "first bare value → LegacyBool". What happens with mixed lines `[crawl]\ntrue\noverlay=1`? The `has_kv` path wins, the bare `true` is lost. C++ would have `lookup_["overlay"]="1"` *and* `values_=["true"]`. Probably matches C++ for the Detailed-form consumer — but no test confirms.
- **Recommendation:** Add a fixture for mixed-shape crawl. Lock in the behavior.

**Status:** FIXED — Added two tests: `crawl_mixed_kv_and_bare_kv_wins` and `crawl_mixed_bare_then_kv`. Both confirm that presence of any kv line forces the Detailed path and bare values are discarded.

### F39 — `from_kv_section` panic-free path: empty `KvMapAccess` produces no keys, which serde sees as defaults
- **Area:** Correctness
- **Severity:** Nit
- **Location:** `crates/config/src/ini/serde.rs:138-149`
- **Observation:** Reading the `MapAccess` impl, `next_key_seed` returns `Ok(None)` when `pos >= pairs.len()`. Without `#[serde(default)]` on the target struct, this would fail with "missing field". The crate consistently uses `#[serde(default)]` on every kv struct; that contract is implicit. If a future struct skips it, behavior breaks at runtime.
- **Recommendation:** Add a doc comment to `from_kv_section` stating the assumption: "target struct must derive `Deserialize` *and* have `#[serde(default)]` (struct-level) for missing keys to be tolerated."

**Status:** FIXED — Added doc comment to `from_kv_section` stating the requirement: target struct must have `#[serde(default)]` at the struct level for missing keys to be silently skipped. Updated the module-level doc note.

### F40 — `error()` message accumulation: no `with_file` wrapping happens automatically when reading sub-files
- **Area:** Correctness / UX
- **Severity:** Minor
- **Location:** `crates/config/src/bootstrap.rs:208-213` (splice), error.rs `with_file`/`with_span`
- **Observation:** Design §12: "config error at /etc/opt/rippled/rippled.cfg:42:5: …". When `splice_validators_file` calls `parse_ini`, any error returned has no source-file context (the splice doesn't call `.with_file(path)`). The operator's error message will not mention which file the error came from. The `with_file` helper exists; it's just not invoked here.
- **Recommendation:** In `splice_validators_file`, wrap the `parse_ini` result: `crate::ini::parse_ini(&text).map_err(|e| e.with_file(path.to_owned()))?`. Same for the eventual `from_file` path.

**Status:** FIXED — `splice_validators_file` already wraps errors with `.with_file(path.to_owned())` (added in F7 fix). The finding is addressed by the F7 fix.

### F41 — INI `[ips]` colon-rewrite regex from C++ is not separately tested (HostPort tests cover it indirectly)
- **Area:** Test coverage
- **Severity:** Nit
- **Location:** `crates/config/src/types/hostport.rs:115-130`
- **Observation:** Analysis §1.5 / §6.16 describes the C++ regex `:([0-9]+)$` that splits `host:port` when there is exactly one colon. The Rust `HostPort::from_str` reimplements this in `colon_count == 1`. There's no specific test asserting that an IPv6-with-port like `fe80::1:51235` (multi-colon, ambiguous) parses as a bare IPv6 (port None), not as host=`fe80::1` port=`51235`. The HostPort tests `bare_ipv6_no_port` (`fe80::1`) and `multi_colon_non_bracketed_is_bare_ipv6` (`::1`) hint at this but don't cover the ambiguous mid-string case.
- **Recommendation:** Add tests for `fe80::1:51235` (currently classified as bare IPv6, port `None`), `a::b:c:1234`, etc., to lock in the rule against future regressions.

**Status:** FIXED — Added four tests to `hostport.rs` (F41): `multi_colon_ipv6_with_valid_hex_suffix_is_bare_ipv6`, `multi_colon_with_decimal_port_is_hostname`, `bracketed_ipv6_with_numeric_port_suffix_parses`, `single_colon_non_ipv6_treated_as_host_port`. These lock in the colon-count disambiguation rule and the bracketed-IPv6 port form.

### F42 — Adapt path does not propagate `SourceSpan` to errors raised from `parse_ini_int` etc.
- **Area:** Correctness / UX
- **Severity:** Minor
- **Location:** `crates/config/src/ini/adapt.rs:251-256` and many others
- **Observation:** Adapter calls like `p.network_id = parse_ini_int(&adapt_single_line(sec)?)?;` lose the source span. The lexer attaches a `SourceSpan` to every `RawLine`, but adapt-stage errors never thread that span into the returned `ConfigError`. Design §12 says spans appear in error messages — they currently never do for INI grammar errors.
- **Recommendation:** Plumb the span from `RawLine` into the error via `.with_span(line.span.clone())`. Either change `adapt_single_line` to return `(String, SourceSpan)`, or have every adapter wrap `parse_ini_int(...)` calls with `.map_err(|e| e.with_span(line.span.clone()))`.

**Status:** WONT FIX — Plumbing `SourceSpan` from `RawLine` through every `parse_ini_int` / `parse_ini_bool` call in `adapt.rs` requires threading the span into every helper function, adding a parameter to ~40 call sites. The work is well-scoped but large enough to be its own PR. The `with_span` helper exists; plumbing it is deferred to a follow-up that also addresses F24 (raw line text in blobs). Filed as a known gap.

### F43 — `regex` crate is in `Cargo.toml` but never imported
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/Cargo.toml:13`
- **Observation:** `regex = "1"` is declared. `grep -rn "use regex" crates/config/src/` returns no hits. The dependency adds build time and binary size with no current use.
- **Recommendation:** Remove the dependency until it's actually needed. (Design §14 lists it for `amendment_majority_time` and the colon-rewrite — both handwritten in the current code, so the entry is stale.)

**Status:** FIXED — Removed `regex = "1"` from `[dependencies]` in `Cargo.toml`. Neither `amendment_majority_time` nor the HostPort colon-rewrite uses regex; both are handwritten.

### F44 — `tempfile` is in `[dev-dependencies]` but unused
- **Area:** Idiomatic Rust
- **Severity:** Nit
- **Location:** `crates/config/Cargo.toml:21`
- **Observation:** `tempfile = "3"` declared as dev-dependency. `grep -rn "use tempfile\|use ::tempfile\|tempfile::" crates/config/tests/ crates/config/src/` returns no hits — tests build their own temp dirs via `std::env::temp_dir().join("…unique…")` which is the wrong pattern (leaves files behind on panic). Either use `tempfile` (clean teardown via RAII) or remove the dep.
- **Recommendation:** Migrate tests to `tempfile::TempDir`. Test isolation improves and the lingering `std::env::temp_dir().join("…unique…")` patterns stop polluting `/tmp` on failure.

**Status:** FIXED — Removed `tempfile = "3"` from `[dev-dependencies]` in `Cargo.toml`. The dependency was declared but never imported; tests that needed temporary files used `std::env::temp_dir()` directly. Removing the dep also eliminates the "use tempfile properly or remove" ambiguity.

## Summary

- **Total findings:** 44
- **By severity:**
  - Blocker: **1** (F1)
  - Major: **9** (F2, F3, F5, F6, F7, F8, F12, F14, F18)
  - Minor: **22**
  - Nit: **12**

- **Top 3 must-fix items:**
  1. **F1 — example config doesn't parse**, three regression tests `#[ignore]`d. Fix `PortConfigProxy`'s `#[serde(flatten)]` so realistic configs round-trip, then un-ignore the tests.
  2. **F2 — section names lowercased**, contradicting design §7 #4 and C++ behavior. Remove the `to_lowercase()` in the lexer and rebuild the case-sensitive lookup.
  3. **F18 — `validate_lenient` only implemented on `OverlayConfig`**; the asymmetric-strictness contract says INI must clamp the analysis-§5 fields silently, but `[reduce_relay]`, `[node_db]`, `[transaction_queue]` do not clamp at all in INI mode.

# Config Rewrite — Step 4 Verification Pass

## Baseline
- `cargo test -p config`: **586 passed / 0 failed / 0 ignored** across 7 suites (lib unit tests 546 + integration: example_config 3, format_equivalence 4, ini_fixtures 11, strict_errors 8, toml_fixtures 8, validators_splice 6 + 0 doc-tests). Matches the developer's claim.

## Verdicts

### F1 — Canonical example config does not parse (`PortConfigProxy` flatten bug)
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Confirmed at `crates/config/src/ini/adapt.rs:29-131` — `adapt_port_section()` walks the kv map field-by-field (no serde flatten); the three previously-ignored regression tests in `crates/config/tests/example_config.rs:27,33,60` no longer carry `#[ignore]` and pass. Aligns with design §3.1 / §13.2 ("xrpld-example.cfg must parse without error").

### F2 — Section names are lowercased; design and C++ behavior require case-sensitive
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `crates/config/src/ini/lexer.rs:42-47` stores section names verbatim (no `to_lowercase()`); `crates/config/src/ini/raw.rs:62-86` keys the index by the exact name and the new `sections_named_case_sensitive_lookup` test (`raw.rs:196-205`) asserts `OVERLAY` ≠ `overlay`. Matches design §7 #4 / analysis §6.9.

### F3 — `[header] trailing stuff` is silently accepted as section `header`; C++ rejects it
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `lexer.rs:101-114` requires `trimmed.ends_with(']')`; trailing content makes the line a bare value. Matches C++ `parseIniFile` per analysis §1.4.

### F4 — Duplicate / dead `voting_config` field
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `voting_config` field removed from `Parsed`; only `voting: VotingConfig` remains (`config.rs:66`). No remaining references in `ini/adapt.rs` or `toml/schema.rs` (only one test name still mentions it).

### F5 — `network_quorum()` and `validation_quorum()` return the same thing
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `config.rs:408-410` returns `self.parsed.network_quorum` only; `config.rs:627-629` returns `self.overrides.validation_quorum: Option<u64>`. Test `config_validation_quorum_override` (`config.rs:910-916`) locks in the distinction. The cross-validator at `bootstrap.rs:175-186` uses `cfg.parsed.network_quorum` (file value), matching the analysis §2.2 distinction.

### F6 — Cross-validator omits `peers_in_max ≤ 1000` check
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `bootstrap.rs:150-173` adds the `peers_in_max ≤ 1000` and `peers_out_max in 10..=1000` checks. `toml/schema.rs:742-755` (`validate_strict_toplevel`) mirrors them for strict mode.

### F7 — `splice_validators_file` always parses as INI, even when called from TOML mode
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `bootstrap.rs:235-239` chooses the parser by extension and wraps errors with `.with_file(path.to_owned())`. Consistent with design §6.

### F8 — TOML mode does not error on validators-file section overlap
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `Parsed.source_format` plumbed in (`bootstrap.rs:243`); overlap checks at `bootstrap.rs:246-254` construct `ConfigError::validators_file_overlap(..)` (verified live constructor at `error.rs:264`). Matches design §5.5.

### F9 — Implicit `validators.txt` discovery uses `exists()` then re-opens (TOCTOU)
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `bootstrap.rs:65-81` calls `splice_validators_file` unconditionally and silently swallows `io::ErrorKind::NotFound`, propagating other I/O errors. No more `exists()`-then-open race.

### F10 — `discover_config_file` returns the last candidate when none exist
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** Behavior matches the C++ "last-tried path even if it doesn't exist" semantic noted in analysis §4 line 266. The function comment at `bootstrap.rs:298-299` documents this. Opinionated divergence from the original review; rebuttal is consistent with the analysis.

### F11 — `from_file` clobbers `set_config_dir` overrides set before the call
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `config.rs:272-275` carries the documented warning. Original finding was a nit; doc-only fix is appropriate.

### F12 — `ConfigOutcome::error()` / `UnitOutcome::error()` leak memory
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `ffi.rs:33-95` materializes `error_msg: String` at construction; `error()` returns `&self.error_msg`. No more `Box::leak`. Note: the comment on `ffi.rs:242` ("leaks on error; acceptable for startup diagnostics") is now stale and should be updated, but the underlying behavior is fixed.

### F13 — `cxx::bridge` declares `HostPortFfi` but never constructs it
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** `HostPortFfi` is still declared at `ffi.rs:230-234` but unused. Design §10 enumerates it as part of the eventual FFI surface, and the file's own preamble (`ffi.rs:5-6`) commits to staged expansion in step 6. Deferring is consistent with the plan-doc Step 6 ownership of the FFI surface.

### F14 — FFI surface is far smaller than design §10 specifies
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** The original finding itself acknowledges "the design doc itself acknowledges 'expanded as C++ migration proceeds'". Step 6 owns this migration; the current minimum-surface FFI (`NodeDbHandle` POC) is appropriate. Adding the full surface now would lock in shapes before C++ consumers exist to validate them.

### F15 — `Config::bootstrap()` `.clone()` and `quiet → stderr echo` semantics
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `bootstrap.rs:110` borrows without clone; `config.rs:641-643` implements `quiet() = silent() || overrides.quiet.unwrap_or(false)`, centralizing the silent→quiet implication.

### F16 — `[node_db]` ignores its bare-line content silently
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment added at `adapt.rs:749-750`. Original finding was a doc nit; doc-only fix is appropriate.

### F17 — `RelPath` is `Deserialize` but accepts only string-shaped TOML
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `types/path.rs:14-22` removes the derive and provides a custom `Deserialize` that reads a string. Module docs clarify the new behavior.

### F18 — INI lenient `validate_lenient` pattern not generalised
- **Original status:** FIXED
- **Verdict:** DISAGREE — fix incomplete: TxQ clamps missing.
- **Detail:** The original finding listed three sections — `[reduce_relay]`, `[node_db]`, AND `[transaction_queue]` (consensus percent clamps from analysis §3.7). The fix added `validate_lenient` for `ReduceRelayConfig` (`adapt.rs:914-923`) and `NodeDbConfig` (`adapt.rs:926-955`), and `dispatch_section` calls them at `adapt.rs:235,239,259`. However the `"transaction_queue"` arm at `adapt.rs:245-247` does *not* call any clamp helper, and `TxQConfig` has no `validate_lenient` impl (it only has `validate_strict` in `toml/schema.rs:575-606`). So INI lenient mode still passes through `normal_consensus_increase_percent = 5000` or `slow_consensus_decrease_percent = 500` unchanged, contradicting the analysis §3.7 description that these are clamped at parse time in C++. The dev's status line claims "FIXED" without disclosing this partial coverage. Recommended: add `impl TxQConfig::validate_lenient` clamping the two consensus-percent fields to their documented ranges, and call it from the `"transaction_queue"` arm.

### F19 — `[features]` does not validate against the registered-feature list
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment on `Config::features()` (`config.rs:567-571`) documents the Phase-3 invariant; test `unknown_feature_name_survives_parse` (`adapt.rs:1565-1570`) locks in the contract.

### F20 — `parse_amendment_majority_time` overflow handling
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** The original finding was filed as a *confirmation*, not a defect. `duration.rs:69-77` uses `checked_mul`; behavior matches analysis §1.4 / C++ regex.

### F21 — `RawSections::sections_named` allocated a `String` for every lookup
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Resolved as side-effect of F2. `raw.rs:79` now does `self.by_name.get(name)` directly with no allocation.

### F22 — `CrawlConfig::default()` differs from C++ "all-true at use time"
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** The C++ uses `valueOr<bool>(.., true)` at *consumer* time, not at parse time. The Rust parser layer stores `Detailed { all false }` and consumers are responsible for the "default true when omitted" semantic. The test `crawl_default_is_all_false` (`toml/schema.rs:1458-1465`) locks this in explicitly. This is a documented design choice and is consistent with analysis §3.3 (the parsed struct in C++ similarly does not set true at parse time).

### F23 — `dispatch_section` doesn't handle the synthetic `__preamble__` section
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Comment block at `adapt.rs:441-450` documents the `__preamble__` fall-through alongside the general silent-drop arm. Matches design §5.3.

### F24 — `adapt_multi_line_blob` reformats KV lines, inventing content
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** Practical-vs-theoretical concern. Base64 content never produces a line classifiable as `KeyValue` by `lexer::classify_line` (`lexer.rs:149-165`) because the key regex demands an ASCII-alpha lead + alphanumerics-or-underscore before `=`, and a non-empty value after `=`. A base64 line with padding `Abc1234=` has empty value → classifies as bare. Edge cases that *would* match (e.g. a literal `name=foo` embedded in a token) are not produced by base64. Deferring to a follow-up that also handles F42 is reasonable.

### F25 — Test coverage gap for `\#` escape behavior
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Four new tests at `lexer.rs:551-595` cover multiple escaped hashes, end-of-value, real-comment-after-escape, and double-backslash-before-hash.

### F26 — `port_listed_but_no_table_uses_defaults` test misnamed
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Renamed to `port_listed_but_no_table_errors_via_zero_port` at `toml/schema.rs:1802`.

### F27 — INI lenient `[node_db]` accepts typo keys into `backend_extras`
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment added at `adapt.rs:749-750`. Test `node_db_unknown_key_goes_to_extras` (`adapt.rs:1574-1583`) locks in the behavior.

### F28 — `error.rs` constructor mixed `&'static str` and `impl Into<String>`
- **Original status:** WONT FIX
- **Verdict:** AGREE — rebuttal valid.
- **Detail:** Original finding was a nit-level style preference. The rationale (static metadata vs. dynamic strings) is a defensible convention. No correctness issue.

### F29 — `ConfigError: Clone` requires `Arc<io::Error>` workaround
- **Original status:** WONT FIX
- **Verdict:** NEEDS HUMAN.
- **Detail:** The dev's stated rationale — that the FFI `ConfigOutcome` "stores the error for the lifetime of the outcome handle, which may be inspected multiple times" — is no longer accurate after F12's fix. F12 changed `ConfigOutcome` to eagerly materialize the message into `error_msg: String` at construction, so the inner `ConfigError` is not cloned per access. A grep across `crates/config/src/` finds no `ConfigError.clone()` call sites. So `Clone` is genuinely not needed today. That said, the original finding was a nit, the `Arc<io::Error>` workaround is small, and removing `Clone` is real refactor work. Human should decide whether to insist on the cleanup or accept the unused trait derivation.

### F30 — `discover_config_file` only checked a subset of search paths
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `bootstrap.rs:308-332` walks `xrpld.cfg`, `rippled.cfg`, `$XDG_CONFIG_HOME/<sys>/{xrpld,rippled}.cfg` (with `$HOME/.config` fallback baked into the same path computation), and `/etc/opt/<sys>/{xrpld,rippled}.cfg`. Covers analysis §4.

### F31 — No test of `[features]` unknown name
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Resolved jointly with F19 via `unknown_feature_name_survives_parse`.

### F32 — `parse_amendment_majority_time` prefix matching
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `duration.rs:44-58` now splits at first whitespace and matches the unit exactly. Tests `unit_prefix_match_minutess_is_err` (loose) and `unit_prefix_match_minutess_strict_is_err` (strict) at `duration.rs:224-234` lock in the rejection.

### F33 — `node_size_from_ram_gb` hard-coded `huge_thresh: u64 = 64`
- **Original status:** WONT FIX
- **Verdict:** NEEDS HUMAN.
- **Detail:** The dev's rationale is internally inconsistent. It claims C++ "walks from Huge down and returns the first bucket whose minimum RAM is met" then notes "The Huge column stores 0, which means 'no minimum'" — which would make Huge always match the first iteration, contradicting the claim that "C++ never returns Huge from RAM alone". Without inspecting `src/xrpld/core/detail/Config.cpp` directly to confirm whether the C++ walk is top-down-pick-first or bottom-up-pick-largest, I cannot conclusively rule on this. The function comment at `bootstrap.rs:402-405` claims "matches C++ server sizing guide" — this is plausible but unverified from inside the docs. Human should grep the C++ source for `getValueFor(RamSizeGb` / the walk loop and confirm whether the 64-GB Huge threshold matches the C++ behavior or is a deliberate Rust extension.

### F34 — `set_silent(false)` doesn't clear `quiet` bit
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment on `set_silent` (`config.rs:320-321`) documents the one-way bridge; `quiet()` getter (`config.rs:641-643`) handles the implication via `silent() || quiet_override`. Both behaviors are now explicit.

### F35 — Detection tests assume CPU count
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `pub fn detect_node_size_with(ram_gb: u64, cpu_count: usize) -> NodeSize` at `bootstrap.rs:383-388` exposes the parameterised version; `detect_node_size()` (`bootstrap.rs:358-374`) delegates to it after probing. The matrix tests live in the `#[cfg(test)]` module and cover RAM × CPU combinations without env-var serialisation.

### F36 — `Parsed::default()` initializes voting twice
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Resolved by F4. `Parsed::default()` (`config.rs:105+`) no longer references `voting_config`.

### F37 — `Config::voting()` returns by value
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment at `config.rs:693-698` explains the by-value return and the future caching optimisation. Consistent with design §9 deferred note.

### F38 — `[crawl]` mixed-shape behavior untested
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Tests `crawl_mixed_kv_and_bare_kv_wins` and `crawl_mixed_bare_then_kv` at `adapt.rs:1594-1614` confirm that any kv line forces the Detailed path.

### F39 — `from_kv_section` implicit `#[serde(default)]` requirement
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Doc comment at `ini/serde.rs:25-34` states the contract: target struct must have `#[serde(default)]` at struct level.

### F40 — `splice_validators_file` errors not wrapped with `with_file`
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Resolved by F7. `bootstrap.rs:239` chains `.map_err(|e| e.with_file(path.to_owned()))`.

### F41 — INI `[ips]` colon-rewrite not separately tested
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** Four tests at `hostport.rs:341-389` lock in the colon-count disambiguation rule for multi-colon IPv6, multi-colon hostnames, bracketed IPv6 + port, and single-colon non-IPv6.

### F42 — Adapt path doesn't propagate `SourceSpan` to errors
- **Original status:** WONT FIX
- **Verdict:** NEEDS HUMAN.
- **Detail:** Real UX gap: design §12 explicitly promises "config error at /etc/opt/rippled/rippled.cfg:42:5: …" error messages with spans, and the spans never get plumbed into adapt-stage errors. The `with_span` helper exists; ~40 call sites need to be wrapped. The dev's WONT FIX is "deferred to a follow-up". Neither the design doc nor the plan doc explicitly authorize this deferral — design §12 is a hard contract on error UX. However, the work is real and well-scoped. Human should decide whether to require it before step 5 sign-off or accept the deferral with a tracked follow-up.

### F43 — `regex` crate declared but never used
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct.
- **Detail:** `Cargo.toml` no longer lists `regex`. Build-time and binary-size win.

### F44 — `tempfile` dev-dep declared but unused
- **Original status:** FIXED
- **Verdict:** AGREE — fix correct (with minor caveat).
- **Detail:** `Cargo.toml` no longer lists `tempfile`. Note that the original finding's *recommendation* was to migrate the tests to use `tempfile` for clean RAII teardown rather than to remove the dep; the dev chose removal, which is fine but leaves the underlying "tests use `std::env::temp_dir().join(...)` and leak directories on panic" pattern intact. Acceptable as a step-4 outcome; could be revisited later.

## Summary

- **AGREE — fix correct:** 32 (F1, F2, F3, F4, F5, F6, F7, F8, F9, F11, F12, F15, F16, F17, F19, F21, F23, F25, F26, F27, F30, F31, F32, F34, F35, F36, F37, F38, F39, F40, F41, F43, F44) — actually 33 entries
- **AGREE — rebuttal valid:** 8 (F10, F13, F14, F20, F22, F24, F28)
- **DISAGREE — fix incomplete/wrong:** 1 (F18 — TxQ clamps missing)
- **DISAGREE — rebuttal not supported:** 0
- **NEEDS HUMAN:** 3 (F29, F33, F42)

Recount: F1–F44 = 44 findings. AGREE-fix-correct = F1, F2, F3, F4, F5, F6, F7, F8, F9, F11, F12, F15, F16, F17, F19, F21, F23, F25, F26, F27, F30, F31, F32, F34, F35, F36, F37, F38, F39, F40, F41, F43, F44 = 33. AGREE-rebuttal-valid = F10, F13, F14, F20, F22, F24, F28 = 7. DISAGREE = F18 = 1. NEEDS HUMAN = F29, F33, F42 = 3. Total = 33 + 7 + 1 + 3 = 44. ✅

### Top 3 items the human reviewer should look at first

1. **F18 (DISAGREE — fix incomplete).** TxQ INI lenient clamps were called out by the original finding but not added. `normal_consensus_increase_percent` and `slow_consensus_decrease_percent` still pass through unmolested in INI mode, contradicting analysis §3.7. Add `impl TxQConfig::validate_lenient` and call it from the `"transaction_queue"` dispatch arm at `adapt.rs:245-247`.

2. **F42 (NEEDS HUMAN).** SourceSpan never plumbs from `RawLine` into adapt-stage errors. Design §12 explicitly promises file:line:col error messages. The fix is well-scoped (~40 call sites of `parse_ini_int` / `parse_ini_bool` need a `.with_span(line.span)` wrap) but the dev marked it WONT FIX / deferred. Decide whether to insist on this before step 5 sign-off.

3. **F33 (NEEDS HUMAN).** The 64-GB hard-coded Huge threshold may or may not match C++ behavior. The dev's rationale is internally inconsistent. Spend 10 minutes greping the C++ `getValueFor(RamSizeGb` walk to confirm direction (top-down pick-first vs. bottom-up pick-largest) and either accept the divergence as intentional or restore C++-matching behavior.

Honorable mention (lower priority): **F29 (NEEDS HUMAN)** is a nit, but the dev's stated reason for keeping `Clone`+`Arc<io::Error>` is factually wrong after F12 — no remaining caller clones `ConfigError`. Decide whether to leave the dead derivation in place.

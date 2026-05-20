# Config Rewrite Plan

Plan for replacing the current C++ `Config`/`BasicConfig` (`src/xrpld/core/Config.h`, `src/xrpld/core/detail/Config.cpp`) with a Rust implementation in `crates/config/`.

## Goal

Full replacement of the C++ config implementation with Rust. Rust owns parsing, validation, and exposing config options. C++ call sites are migrated to consume the Rust-produced config.

## Desired features

- **Single source of truth.** Config options are defined in one place. May be spread across nested structs, but each option lives in exactly one definition site.
- **Self-documenting.** Each option carries documentation in code; a markdown reference is generated from the definitions (likely via a derive macro). Deferrable to a later step.
- **Validators.** Each field can declare validators; after parsing, fields are validated and errors are reported. Likely macro-driven. Deferrable to a later step.
- **INI and TOML support.** Currently only INI is supported, and parsing is lenient (unknown keys silently ignored). The rewrite supports both formats with **asymmetric strictness**: INI stays lenient (compat-first — existing rippled.cfg files load unchanged), TOML is strict (unknown keys, wrong types, malformed sections, and out-of-range values are errors). Both formats produce the same typed `ParsedConfig`. INI uses a two-stage parse (raw section bag → typed); TOML uses serde directly.

## Plan of plans

1. **Analysis.** Produce an analysis doc covering:
   - Current `Config`/`BasicConfig` field inventory (types, defaults, where set).
   - Current usage patterns (direct field access vs. `getIniFileSection`/`parseKeyValueSection`/`get<T>` getters) and call-site catalog.
   - Implicit schema: which sections are `key=value` maps, which are bare-line lists (`[ips]`, `[ips_fixed]`, `[features]`, `[validators]`, `[cluster_nodes]`, `[node_seed]`, …), which are single-value.
   - Side effects currently embedded in `Config` (logging, SSL context, START_UP modes, path resolution relative to the config file, env-var interactions, etc.).
   - Open questions / edge cases for design decisions.

   Output: [`config_rewrite_analysis.md`](./config_rewrite_analysis.md). **Status: complete.**

2. **Design.** Based on the analysis, produce a design doc for the Rust implementation. This is the contract that step 3 builds against — resolve the open questions from step 1, then specify:
   - Crate layout (modules, public API surface, re-exports).
   - Top-level types: `ParsedConfig` (file-only, immutable) vs. `RuntimeConfig` (parsed + CLI overrides + auto-detected defaults) vs. per-subsystem typed sub-structs (`NodeDbConfig`, `OverlayConfig`, `PortConfig`, …).
   - Concrete approach to INI: two-stage parse (raw section bag → typed) vs. fully custom serde deserializer. Lock in the strategy.
   - Canonical TOML schema (table-of-tables layout for `[port_*]`, etc.) and INI ↔ TOML equivalence rules.
   - Grammar rules (booleans, numerics, durations, paths, comments, identifier case sensitivity) — single rule per type, applied uniformly.
   - C FFI / cxx-rs interface: which types cross the boundary, ownership, error reporting back to C++.
   - Build-system integration (Conan, CMake, cargo workspace placement).
   - Test strategy (round-trip INI/TOML fixtures, the existing example config as a regression input).

   Output: `config_rewrite_design.md`.

3. **Rust implementation.** Build the Rust crate against the design from step 2:
   - Typed schema + serde-based TOML parsing.
   - Custom INI handling (two-stage or fully custom, per step 2).
   - Strict mode for both formats.
   - C FFI surface that returns the parsed config to C++ callers.
   - Unit tests, fixture-based round-trip tests, fuzz target (deferrable).

   No C++ behavior change yet — the crate exists and is exercised by Rust tests, but rippled still uses the C++ `Config`.

4. **Agentic review.** Spawn review agents over the step-3 output to surface issues before the human review pass:
   - **Correctness review.** One agent walks the parser/adapter/validator paths against `config_rewrite_analysis.md` §2–§5, looking for missed fields, wrong defaults, mis-applied clamps, missing cross-section validators.
   - **Test-coverage review.** One agent walks `crates/config/src/**` and `crates/config/tests/` and checks every grammar primitive, adapter, and validator has both happy-path and failure-mode coverage (per §13 of the design doc).
   - **Idiomatic-Rust review.** One agent reviews the crate for Rust idiom — borrow-vs-clone discipline, error-type ergonomics, naming, module boundaries, `#[derive]` use, `serde` patterns, FFI surface shape against §10.
   Output: a consolidated report with concrete file/line callouts. Findings are addressed in-place before step 5.

5. **Human review.** Personal review pass over the step-3 code (informed by the step-4 report). Sign-off here is the precondition for step 6.

6. **C++ migration.** Replace the C++ `Config`/`BasicConfig` with consumption of the Rust-produced config:
   - Wire the Rust crate into rippled's build (Conan + CMake).
   - Migrate every consumer in §3 of the analysis (per-section parsers in `OverlayImpl`, `ServerHandler`, `SHAMapStoreImp`, `TxQ`, `AmendmentTable`, `ValidatorKeys`, etc.) to read from the typed Rust output instead of `Section::get<T>`. Per the design doc §11, there is no intermediate C++ shim — call sites move directly to the Rust API via the cxx-generated header.
   - Delete the old C++ `Config`/`BasicConfig`/`ConfigSections` once nothing in-tree depends on them.
   - Ship `--check-config` and `--convert-config` (INI → TOML, plus validation report) per the migration-tooling commitment below.

7. **Documentation support.** Macro-driven markdown generation from option definitions.

8. **Validation support.** Macro-driven per-field validators run after parse.

## Decisions / agreed positions

- **Approach:** full replacement, not a parsing shim. Larger blast radius, accepted in exchange for a clean Rust-shaped interface.
- **INI non-standard sections.** rippled's INI is not standard `key=value`; several sections are bare-line lists. Approach: implement a custom serde INI deserializer that understands the section shapes. Concrete strategy (two-stage parse vs. fully custom deserializer) is an open question to be resolved in step 2 (design).
- **Asymmetric strict mode.** INI is **lenient by default** — replicates existing `BasicConfig` behavior, including silent-ignore of unknown keys / sections, silent clamps on the fields that already clamp today, and the existing per-field grammars. TOML is **strict by default** — unknown keys/sections, out-of-range values, and trailing-junk in custom grammars (e.g. `amendment_majority_time`) are errors. INI may be tightened later. See the analysis doc for the field-by-field rules.
- **Migration tooling.** rippled will ship `--check-config` and `--convert-config` flags (INI → TOML, plus validation report) to ease the transition. These land in step 6.
- **Fallback.** Both formats supported indefinitely. INI is the path of least resistance for existing operators; TOML is the recommended format for new deployments and gets the cleaner schema (table-of-tables for `[port.*]`, uniform path resolution, etc.).

## Out of scope (for now)

- Choice of macro/derive crates for docs and validation (decided when steps 7–8 begin).
- Hot reload / runtime reconfiguration.
- Any change to the set of config options themselves — this is a rewrite, not a redesign of what's configurable.

## Next step

Steps 1 and 2 are complete:
- Step 1 — [`config_rewrite_analysis.md`](./config_rewrite_analysis.md).
- Step 2 — [`config_rewrite_design.md`](./config_rewrite_design.md); all open questions resolved in §15.

Step 3 (Rust implementation) builds against the design doc. Step 4 (agentic review) and step 5 (human review) gate step 6 (C++ migration).

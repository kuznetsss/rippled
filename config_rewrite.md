# Config Rewrite Plan

Plan for replacing the current C++ `Config`/`BasicConfig` (`src/xrpld/core/Config.h`, `src/xrpld/core/detail/Config.cpp`) with a Rust implementation in `crates/config/`.

## Goal

Full replacement of the C++ config implementation with Rust. Rust owns parsing, validation, and exposing config options. C++ call sites are migrated to consume the Rust-produced config.

## Desired features

- **Single source of truth.** Config options are defined in one place. May be spread across nested structs, but each option lives in exactly one definition site.
- **Self-documenting.** Each option carries documentation in code; a markdown reference is generated from the definitions (likely via a derive macro). Deferrable to a later step.
- **Validators.** Each field can declare validators; after parsing, fields are validated and errors are reported. Likely macro-driven. Deferrable to a later step.
- **INI and TOML support.** Currently only INI is supported, and parsing is lenient (unknown keys silently ignored). Both formats will be **strict** — unknown keys, wrong types, and malformed sections are errors. serde drives TOML; a custom serde INI deserializer handles INI (see edge cases below).

## Plan of plans

1. **Analysis.** Produce a design doc covering:
   - Current `Config`/`BasicConfig` field inventory (types, defaults, where set).
   - Current usage patterns (direct field access vs. `getIniFileSection`/`parseKeyValueSection`/`get<T>` getters) and call-site catalog.
   - Implicit schema: which sections are `key=value` maps, which are bare-line lists (`[ips]`, `[ips_fixed]`, `[features]`, `[validators]`, `[cluster_nodes]`, `[node_seed]`, …), which are single-value.
   - Side effects currently embedded in `Config` (logging, SSL context, START_UP modes, path resolution relative to the config file, env-var interactions, etc.).
   - Open questions / edge cases for design decisions.
2. **Basic implementation.** Rust crate that parses both INI and TOML into the typed config struct(s). Strict parsing for both formats. C++ consumers migrated over.
3. **Documentation support.** Macro-driven markdown generation from option definitions.
4. **Validation support.** Macro-driven per-field validators run after parse.

## Decisions / agreed positions

- **Approach:** full replacement, not a parsing shim. Larger blast radius, accepted in exchange for a clean Rust-shaped interface.
- **INI non-standard sections.** rippled's INI is not standard `key=value`; several sections are bare-line lists. Approach: implement a custom serde INI deserializer that understands the section shapes. Concrete strategy (two-stage parse vs. fully custom deserializer) is an open question to be resolved in step 1.
- **Strict parsing is a behavior change.** Accepted. Existing `rippled.cfg` files with typos / stale keys / commented experiments that boot today may be rejected.
- **Migration tooling.** rippled will ship `--check-config` and `--convert-config` flags (INI → TOML, plus validation report) to ease the transition.
- **Fallback.** If the community pushes back on strict INI, relax it later. Strict is the default from day one.

## Out of scope (for now)

- Choice of macro/derive crates for docs and validation (decided when steps 3–4 begin).
- Hot reload / runtime reconfiguration.
- Any change to the set of config options themselves — this is a rewrite, not a redesign of what's configurable.

## Next step

Begin step 1: produce the analysis design doc. Output goes alongside this file (e.g. `config_rewrite_analysis.md`) and should end with a list of open questions for review before step 2 starts.

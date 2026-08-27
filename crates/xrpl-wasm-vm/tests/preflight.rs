//! What screening refuses, and that it refuses nothing a run would have served.
//!
//! `check` reaches its verdict from the compiled module alone, so these tests take
//! no host — except the ones that put the same module through `run` to compare the
//! two.

mod support;

use support::{ENTRY, FakeHost, ONE_PAGE, PLENTY_OF_GAS, assemble, import, module};
use xrpl_host_functions::{HOST_MODULE, HostFunctionSpec, WasmValType};
use xrpl_wasm_vm::{CheckError, MAX_MEMORY_PAGES, MAX_TABLE_ELEMENTS, RunError};

/// Assert which stage screening refused a module at, because the caller maps the
/// stages separately. The error comes back out for the tests that also read its
/// message.
macro_rules! assert_stage {
    ($refusal:expr, $stage:pat) => {{
        let refusal = $refusal;
        assert!(
            matches!(refusal, $stage),
            concat!("expected a ", stringify!($stage), " refusal, got: {}"),
            refusal
        );
        refusal
    }};
}

/// Screens `wat`, which must assemble.
fn check(wat: &str) -> Result<(), CheckError> {
    xrpl_wasm_vm::check(&assemble(wat), ENTRY)
}

fn refusal(wat: &str) -> CheckError {
    check(wat).expect_err(&format!("expected this module to be refused:\n{wat}"))
}

fn passes(wat: &str) {
    if let Err(refusal) = check(wat) {
        panic!("expected this module to pass, but: {refusal}\n{wat}");
    }
}

// ---------------------------------------------------------------------------
// Compiling
// ---------------------------------------------------------------------------

/// A contract that imports a host function, exports its memory and exports the
/// entry point is what screening is looking for.
#[test]
fn a_runnable_contract_passes() {
    passes(&module(
        &[import::LDGR_INDEX, ONE_PAGE],
        "(call $ldgr_index (i32.const 0) (i32.const 4))",
    ));
}

/// Bytes that are not a wasm module at all.
#[test]
fn garbage_does_not_pass() {
    for bytes in [b"".as_slice(), b"not wasm", &[0x00, 0x61, 0x73, 0x6d]] {
        let refusal = xrpl_wasm_vm::check(bytes, ENTRY).expect_err("garbage must not pass");
        assert_stage!(refusal, CheckError::Compile(_));
    }
}

/// Screening takes wasm binaries, and text is not one — the same rule the VM
/// applies, from the same `wasmi` built without its `wat` feature. Turning that
/// feature on would make this transaction blob valid at both ends.
#[test]
fn a_text_format_module_does_not_pass() {
    let text = module(&[ONE_PAGE], "(i32.const 0)");

    let refusal =
        xrpl_wasm_vm::check(text.as_bytes(), ENTRY).expect_err("text must not pass as a module");
    assert_stage!(refusal, CheckError::Compile(_));

    // The same module, assembled first, passes: the text is sound and only the
    // format was refused.
    passes(&text);
}

/// A feature the engine disables is refused here too, because both stages compile
/// against the one engine. `vm_limits.rs` walks every disabled feature; this pins
/// that screening sees the same configuration.
#[test]
fn a_disabled_feature_does_not_pass() {
    let refusal = refusal(&module(
        &[ONE_PAGE],
        "(drop (f64.add (f64.const 1) (f64.const 2))) (i32.const 0)",
    ));
    let refusal = assert_stage!(refusal, CheckError::Compile(_)).to_string();
    assert!(refusal.contains("floating-point"), "{refusal}");
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

/// Every host function the ABI declares, spelled as a guest imports it. The count
/// is asserted against the ABI so a function added to it cannot be left out here.
const ALL_IMPORTS: [&str; 61] = [
    import::LDGR_INDEX,
    import::PARENT_LDGR_TIME,
    import::PARENT_LDGR_HASH,
    import::BASE_FEE,
    import::AMENDMENT_ENABLED,
    import::CACHE_LE,
    import::TX_FIELD,
    import::HOME_LE_FIELD,
    import::LE_FIELD,
    import::TX_INNER,
    import::HOME_LE_INNER,
    import::LE_INNER,
    import::TX_ARR_LEN,
    import::HOME_LE_ARR_LEN,
    import::LE_ARR_LEN,
    import::TX_INNER_ARR_LEN,
    import::HOME_LE_INNER_ARR_LEN,
    import::LE_INNER_ARR_LEN,
    import::CHECK_SIG,
    import::ACCOUNTROOT_ID,
    import::AMM_ID,
    import::CHECK_ID,
    import::CREDENTIAL_ID,
    import::DELEGATE_ID,
    import::DEPOSIT_PREAUTH_ID,
    import::DID_ID,
    import::ESCROW_ID,
    import::TRUSTLINE_ID,
    import::MPT_ISSUANCE_ID,
    import::MPTOKEN_ID,
    import::NFT_OFFER_ID,
    import::OFFER_ID,
    import::ORACLE_ID,
    import::PAYCHAN_ID,
    import::PERMISSIONED_DOMAIN_ID,
    import::SIGNERS_ID,
    import::TICKET_ID,
    import::VAULT_ID,
    import::SHA512_HALF,
    import::TRACE,
    import::SET_DATA,
    import::NFT_URI,
    import::NFT_ISSUER,
    import::NFT_TAXON,
    import::NFT_FLAGS,
    import::NFT_XFER_FEE,
    import::NFT_SERIAL,
    import::FLOAT_FROM_INT,
    import::FLOAT_FROM_UINT,
    import::FLOAT_FROM_STAMOUNT,
    import::FLOAT_FROM_STNUMBER,
    import::FLOAT_TO_INT,
    import::FLOAT_TO_MANT_EXP,
    import::FLOAT_FROM_MANT_EXP,
    import::FLOAT_CMP,
    import::FLOAT_ADD,
    import::FLOAT_SUB,
    import::FLOAT_MULT,
    import::FLOAT_DIV,
    import::FLOAT_ROOT,
    import::FLOAT_POW,
];

/// The 61 imports above are written by hand, so putting them through the signature
/// check compares them against the table the ABI derives from its declarations —
/// two statements of the wire, one of them not generated from the other.
#[test]
fn every_declared_host_function_may_be_imported() {
    assert_eq!(
        ALL_IMPORTS.len(),
        HostFunctionSpec::ALL.len(),
        "the ABI gained a host function with no import declaration in this test"
    );

    let mut parts = ALL_IMPORTS.to_vec();
    parts.push(ONE_PAGE);
    passes(&module(&parts, "(i32.const 0)"));
}

/// One `(import …)` for `function`, typed as [`HostFunctionSpec::wasm_params`] and
/// [`HostFunctionSpec::wasm_result`] report it. Unnamed: nothing calls these, and
/// linking resolves an import by its module and name.
fn declared_import(function: HostFunctionSpec) -> String {
    fn to_str(ty: WasmValType) -> &'static str {
        match ty {
            WasmValType::I32 => "i32",
            WasmValType::I64 => "i64",
        }
    }

    let params = match function.wasm_params() {
        [] => String::new(),
        params => format!(
            " (param {})",
            params
                .iter()
                .map(|ty| to_str(*ty))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    };
    let result = match function.wasm_result() {
        None => String::new(),
        Some(ty) => format!(" (result {})", to_str(ty)),
    };

    format!(
        r#"(import "{HOST_MODULE}" "{name}" (func{params}{result}))"#,
        name = function.wasm_name()
    )
}

/// **The probe.** A module whose imports are spelled from the ABI's signature table,
/// instantiated against the engine that registers them: if the two disagree about
/// any of the 61, linking refuses the module and this fails.
///
/// It is the only test that covers the whole ABI's wire shape at once, and the only
/// one that reaches the guest's side of instantiation — the module name and all 61
/// types together. `run` rather than `check`, because instantiation is where an
/// import's type is matched; screening it too is what pins that the earlier stage
/// agrees.
#[test]
fn a_module_importing_every_host_function_runs() {
    let imports: Vec<String> = HostFunctionSpec::ALL
        .iter()
        .copied()
        .map(declared_import)
        .collect();
    let mut parts: Vec<&str> = imports.iter().map(String::as_str).collect();
    parts.push(ONE_PAGE);

    let wat = module(&parts, "(i32.const 0)");

    passes(&wat);
    assert_eq!(
        support::run(&wat, &FakeHost::new())
            .expect("a module importing the declared signatures must link")
            .result,
        0
    );
}

/// A module may import fewer host functions than are registered, but not more.
#[test]
fn an_unknown_host_function_does_not_pass() {
    let refusal = refusal(&module(
        &[
            r#"(import "host_lib" "no_such_function" (func $f (param i32) (result i32)))"#,
            ONE_PAGE,
        ],
        "(call $f (i32.const 0))",
    ));
    let refusal = assert_stage!(refusal, CheckError::Import(_)).to_string();
    assert!(
        refusal.contains("no host function 'no_such_function'"),
        "{refusal}"
    );
}

/// Host functions live under one module name — `host_lib` — and an import naming
/// another is refused even when the function name is real. `env` is in the list
/// because that is what plain clang emits.
#[test]
fn an_import_from_another_module_does_not_pass() {
    for module_name in ["host", "env", ""] {
        let refusal = refusal(&module(
            &[
                &format!(
                    r#"(import "{module_name}" "ldgr_index" (func $f (param i32 i32) (result i32)))"#
                ),
                ONE_PAGE,
            ],
            "(call $f (i32.const 0) (i32.const 4))",
        ));
        let refusal = assert_stage!(refusal, CheckError::Import(_)).to_string();
        assert!(refusal.contains("is not from 'host_lib'"), "{refusal}");
    }
}

/// A host function's name imported as something other than a function. The engine
/// defines it as a function and nothing else, so this does not link either.
#[test]
fn a_host_function_imported_as_a_global_does_not_pass() {
    let refusal = refusal(&module(
        &[
            r#"(import "host_lib" "ldgr_index" (global $g i32))"#,
            ONE_PAGE,
        ],
        "(global.get $g)",
    ));
    let refusal = assert_stage!(refusal, CheckError::Import(_)).to_string();
    assert!(
        refusal.contains("'host_lib::ldgr_index' is not a function"),
        "{refusal}"
    );
}

/// A module faulty at two stages is refused by the earlier one — it imports what no
/// engine serves *and* exports no entry point. The imports are what the rest of the
/// module depends on, so that is the message worth having.
#[test]
fn the_earlier_stage_is_the_one_reported() {
    let refusal = refusal(
        r#"(module
             (import "host_lib" "no_such_function" (func $f (result i32)))
             (memory (export "memory") 1)
             (func (export "not_the_entry_point") (result i32) (call $f)))"#,
    );

    assert_stage!(refusal, CheckError::Import(_));
}

/// An import of the right name with the wrong type is refused, and the refusal names
/// both shapes.
///
/// **Every way a signature can be wrong** — the same five `vm_limits.rs`'s
/// `an_import_with_the_wrong_signature_fails_instantiation` walks, so the two stages
/// are held to one enumeration rather than to whichever cases each file happened to
/// pick.
///
/// Each is then run, which is what shows what the check moved: none of these modules
/// could ever have run — instantiation refuses them too — so screening turns away
/// nothing runnable. What changed is *which stage* refuses it, and so which TER a
/// caller maps it to.
#[test]
fn an_import_with_the_wrong_signature_does_not_pass() {
    for (signature, found) in [
        ("(param i32) (result i32)", "(i32) -> i32"),
        ("(param i32 i32 i32) (result i32)", "(i32, i32, i32) -> i32"),
        ("(param i64 i64) (result i32)", "(i64, i64) -> i32"),
        ("(param i32 i32) (result i64)", "(i32, i32) -> i64"),
        ("(param i32 i32)", "(i32, i32) -> ()"),
    ] {
        let wat = module(
            &[
                &format!(r#"(import "host_lib" "ldgr_index" (func $f {signature}))"#),
                ONE_PAGE,
            ],
            "(i32.const 0)",
        );

        let refusal = assert_stage!(refusal(&wat), CheckError::Import(_)).to_string();
        assert!(
            refusal.contains(&format!("expected '(i32, i32) -> i32', found '{found}'")),
            "{signature}: {refusal}"
        );

        let host = FakeHost::new();
        let failure = xrpl_wasm_vm::run(&assemble(&wat), PLENTY_OF_GAS, &host, ENTRY)
            .expect_err("a mistyped import does not link either");
        assert!(
            matches!(failure.error, RunError::Instantiate(_)),
            "{signature}: {failure}"
        );
    }
}

// ---------------------------------------------------------------------------
// The entry point
// ---------------------------------------------------------------------------

#[test]
fn a_missing_entry_point_does_not_pass() {
    let refusal = refusal(
        r#"(module (memory (export "memory") 1)
                              (func (export "other") (result i32) (i32.const 0)))"#,
    );
    let refusal = assert_stage!(refusal, CheckError::EntryPoint(_)).to_string();
    assert_eq!(refusal, "no entry point 'finish'");
}

/// The entry point is looked up by the name the caller asks for, as a run looks it
/// up: screening a contract for one entry point says nothing about another.
#[test]
fn the_entry_point_is_the_name_the_caller_gives() {
    let wasm = assemble(
        r#"(module (memory (export "memory") 1)
             (func (export "other") (result i32) (i32.const 0)))"#,
    );

    assert!(xrpl_wasm_vm::check(&wasm, "other").is_ok());
    assert!(xrpl_wasm_vm::check(&wasm, ENTRY).is_err());
}

/// Both halves of the entry point's type are screened: a module returning the
/// wrong thing, or taking anything at all, would fail the run's typed lookup.
#[test]
fn an_entry_point_of_the_wrong_type_does_not_pass() {
    for (signature, body) in [
        ("(result i64)", "(i64.const 0)"),
        ("(param i32) (result i32)", "(i32.const 0)"),
        ("", "(nop)"),
    ] {
        let refusal = refusal(&format!(
            r#"(module (memory (export "memory") 1)
                 (func (export "finish") {signature} {body}))"#
        ));
        let refusal = assert_stage!(refusal, CheckError::EntryPoint(_)).to_string();
        assert_eq!(
            refusal, "entry point 'finish' has the wrong signature, expected '() -> i32'",
            "{signature}"
        );
    }
}

/// An export of the entry point's name that is not a function at all is a third
/// case, and named as such: nothing is missing and no signature is wrong.
#[test]
fn an_entry_point_that_is_not_a_function_does_not_pass() {
    let refusal = refusal(
        r#"(module (memory (export "memory") 1) (global (export "finish") i32 (i32.const 0)))"#,
    );
    let refusal = assert_stage!(refusal, CheckError::EntryPoint(_)).to_string();
    assert_eq!(refusal, "export 'finish' is not a function");
}

// ---------------------------------------------------------------------------
// Agreement with a run
// ---------------------------------------------------------------------------

/// A module with no linear memory to export passes. A contract that makes no host
/// call needs none, and one that does is refused at the call and charged — a
/// runtime fault, not a malformed module.
#[test]
fn a_module_exporting_no_memory_passes() {
    let wat = r#"(module (func (export "finish") (result i32) (i32.const 0)))"#;
    passes(wat);

    let host = FakeHost::new();
    assert_eq!(
        xrpl_wasm_vm::run(&assemble(wat), PLENTY_OF_GAS, &host, ENTRY)
            .expect("a module that calls no host function needs no memory")
            .result,
        0
    );
}

/// Modules spanning what screening decides, each also put through a run.
fn modules() -> Vec<(&'static str, String)> {
    vec![
        (
            "a runnable contract",
            module(&[import::LDGR_INDEX, ONE_PAGE], "(i32.const 0)"),
        ),
        (
            "a contract that traps",
            module(&[ONE_PAGE], "(unreachable)"),
        ),
        (
            "a disabled feature",
            module(&[ONE_PAGE], "(i32.extend8_s (i32.const 1))"),
        ),
        (
            "an unknown host function",
            module(
                &[
                    r#"(import "host_lib" "nope" (func $f (result i32)))"#,
                    ONE_PAGE,
                ],
                "(call $f)",
            ),
        ),
        (
            "an import from another module",
            module(
                &[
                    r#"(import "env" "ldgr_index" (func $f (param i32 i32) (result i32)))"#,
                    ONE_PAGE,
                ],
                "(i32.const 0)",
            ),
        ),
        (
            "a host function imported as a global",
            module(
                &[r#"(import "host_lib" "trace" (global $g i32))"#, ONE_PAGE],
                "(global.get $g)",
            ),
        ),
        (
            "no entry point",
            r#"(module (memory (export "memory") 1)
                 (func (export "other") (result i32) (i32.const 0)))"#
                .to_string(),
        ),
        (
            "an entry point of the wrong type",
            r#"(module (memory (export "memory") 1)
                 (func (export "finish") (result i64) (i64.const 0)))"#
                .to_string(),
        ),
    ]
}

/// Screening refuses a module exactly when a run would refuse it at one of the
/// three stages screening covers — nothing it rejects would have run, and nothing
/// it passes stops before the entry point is called. The exceptions are the ones
/// [`what_static_screening_cannot_see`] lists.
#[test]
fn screening_and_a_run_agree() {
    let host = FakeHost::new();

    for (label, wat) in modules() {
        let wasm = assemble(&wat);
        let refused_early = match xrpl_wasm_vm::run(&wasm, PLENTY_OF_GAS, &host, ENTRY) {
            Err(failure) => matches!(
                failure.error,
                RunError::Compile(_) | RunError::Instantiate(_) | RunError::EntryPoint(_)
            ),
            Ok(_) => false,
        };

        assert_eq!(
            xrpl_wasm_vm::check(&wasm, ENTRY).is_err(),
            refused_early,
            "{label}"
        );
    }
}

/// A module asking for more memory than the engine grants is refused, so the
/// contract that could never run does not reach the ledger. The cap itself passes.
#[test]
fn an_exported_memory_past_the_cap_does_not_pass() {
    let wat = module(
        &[&format!(
            r#"(memory (export "memory") {})"#,
            MAX_MEMORY_PAGES + 1
        )],
        "(i32.const 0)",
    );
    let refusal = assert_stage!(refusal(&wat), CheckError::Memory(_)).to_string();
    assert!(refusal.contains("past the 128-page cap"), "{refusal}");

    passes(&module(
        &[&format!(r#"(memory (export "memory") {MAX_MEMORY_PAGES})"#)],
        "(i32.const 0)",
    ));
}

/// A declared *maximum* past the cap is legal and simply unreachable, so screening
/// must not turn it away: `vm_limits` runs this very module to completion.
#[test]
fn a_declared_maximum_past_the_cap_still_passes() {
    passes(&module(
        &[&format!(
            r#"(memory (export "memory") 1 {})"#,
            MAX_MEMORY_PAGES + 1
        )],
        "(i32.const 0)",
    ));
}

/// A module asking for more table than the engine grants is refused for the same
/// reason a memory is. The cap itself passes.
#[test]
fn an_exported_table_past_the_cap_does_not_pass() {
    let wat = module(
        &[&format!(
            r#"(table (export "t") {} funcref)"#,
            MAX_TABLE_ELEMENTS + 1
        )],
        "(i32.const 0)",
    );
    let refusal = assert_stage!(refusal(&wat), CheckError::Table(_)).to_string();
    assert!(refusal.contains("past the 1024-element cap"), "{refusal}");

    passes(&module(
        &[&format!(
            r#"(table (export "t") {MAX_TABLE_ELEMENTS} funcref)"#
        )],
        "(i32.const 0)",
    ));
}

/// Both caps are applied in one pass over the exports, so neither may end the walk
/// early: a passing memory must not hide a failing table declared after it, and a
/// passing table must not hide a failing memory.
#[test]
fn one_pass_screens_both_resources() {
    let after_a_passing_memory = refusal(&module(
        &[
            ONE_PAGE,
            &format!(r#"(table (export "t") {} funcref)"#, MAX_TABLE_ELEMENTS + 1),
        ],
        "(i32.const 0)",
    ));
    assert_stage!(after_a_passing_memory, CheckError::Table(_));

    let after_a_passing_table = refusal(&module(
        &[
            r#"(table (export "t") 1 funcref)"#,
            &format!(r#"(memory (export "memory") {})"#, MAX_MEMORY_PAGES + 1),
        ],
        "(i32.const 0)",
    ));
    assert_stage!(after_a_passing_table, CheckError::Memory(_));
}

/// As with memory, a declared *maximum* past the cap is unreachable rather than
/// wrong: `vm_limits` runs this very module to completion.
#[test]
fn a_declared_table_maximum_past_the_cap_still_passes() {
    passes(&module(
        &[&format!(
            r#"(table (export "t") 1 {} funcref)"#,
            MAX_TABLE_ELEMENTS + 1
        )],
        "(i32.const 0)",
    ));
}

/// The gap, listed rather than described. A memory or a table a module keeps to
/// itself is not in its exports, so these are the modules that pass screening and
/// then fail to *instantiate* — which is why a run's refusal at that stage cannot be
/// read as the node's fault.
///
/// The two entries are not equally remote. A contract needs an exported memory to
/// make any host call, so the memory row can do nothing but compute and the SDK does
/// not produce one. A table, though, is *normally* unexported — Rust exports
/// `__indirect_function_table` only under `--export-table` — so the table row is the
/// shape a hostile module actually takes, and the store's limiter is the only thing
/// standing in front of it.
#[test]
fn what_static_screening_cannot_see() {
    let host = FakeHost::new();

    for (label, declaration) in [
        ("memory", format!("(memory {})", MAX_MEMORY_PAGES + 1)),
        (
            "table",
            format!("(table {} funcref)", MAX_TABLE_ELEMENTS + 1),
        ),
    ] {
        let wat = format!(
            r#"(module {declaration}
                 (func (export "finish") (result i32) (i32.const 0)))"#
        );

        passes(&wat);

        let failure = match xrpl_wasm_vm::run(&assemble(&wat), PLENTY_OF_GAS, &host, ENTRY) {
            Err(failure) => failure,
            Ok(outcome) => panic!(
                "the store's limiter must refuse the {label}, but the module returned {}",
                outcome.result
            ),
        };
        assert!(
            matches!(failure.error, RunError::Instantiate(_)),
            "{label}: {failure}"
        );
    }
}

/// A start section runs guest code at instantiation, before the entry point. The
/// engine disallows it, so screening refuses the module outright rather than letting
/// any code run ahead of the entry point.
#[test]
fn a_start_section_is_refused_by_screening() {
    let wat = format!(
        r#"(module {ONE_PAGE}
             (func $init (unreachable))
             (start $init)
             (func (export "finish") (result i32) (i32.const 0)))"#
    );

    let refusal = assert_stage!(refusal(&wat), CheckError::Compile(_)).to_string();
    assert!(refusal.contains("start"), "{refusal}");
}

#[test]
fn a_memory64_memory_is_refused_by_screening() {
    let wat = r#"(module
        (memory i64 1)
        (func (export "finish") (result i32) (i32.const 0)))"#;

    let refusal = assert_stage!(refusal(wat), CheckError::Compile(_)).to_string();
    assert!(
        refusal.contains("memory64") || refusal.contains("i64"),
        "{refusal}"
    );
}

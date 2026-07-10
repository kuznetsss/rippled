//! cxx bridge for the wasm engine. Filled in in Step 3 (C++ -> Rust coarse
//! entry) and Step 4 (Rust -> C++ host-function forwarding).
//!
//! For now this exposes a single coarse entry point, `run_escrow_mocked`,
//! that runs a wasm blob's `finish` export against a built-in
//! [`SampleHost`] (no C++ involved yet). A real `CxxHost` that forwards
//! host calls to C++ replaces `SampleHost` in a later step.
#[cxx::bridge(namespace = "rs::wasm_vm")]
mod ffi {
    /// Mirrors [`crate::RunOutcome`] as a plain-data type cxx can share
    /// across the FFI boundary.
    struct RunResult {
        result: i32,
        fuel_used: u64,
    }

    extern "Rust" {
        // Run `wasm`'s `finish` export with `gas` fuel against a built-in
        // mock host.
        fn run_escrow_mocked(wasm: &[u8], gas: u64) -> Result<RunResult>;

        // Compile WebAssembly text (WAT) to wasm bytes. A tooling/test helper
        // so callers can express modules as readable text instead of raw bytes.
        fn compile_wat(wat: &str) -> Result<Vec<u8>>;
    }
}

use crate::run_escrow;
use host_functions::{HostError, HostFunctions, HostResult, HASH_LEN};

/// Minimal synthetic-ledger host with no external deps (no `sha2`). A real
/// `CxxHost` forwarding to C++ replaces this in the next step.
struct SampleHost;

impl HostFunctions for SampleHost {
    fn get_ledger_sqn(&self) -> HostResult<u32> {
        Ok(7)
    }

    fn get_current_ledger_obj_field(&self, _field: i32) -> HostResult<Vec<u8>> {
        Err(HostError::FieldNotFound)
    }

    fn sha512_half(&self, _data: &[u8]) -> HostResult<[u8; HASH_LEN]> {
        Err(HostError::Internal)
    }

    fn trace(&self, _msg: &str, _data: &[u8], _as_hex: bool) -> HostResult<()> {
        Ok(())
    }

    fn trace_num(&self, _msg: &str, _number: i64) -> HostResult<()> {
        Ok(())
    }
}

/// Runs `wasm`'s `finish` export with `gas` fuel against [`SampleHost`].
///
/// `run_escrow` returns `Result<RunOutcome, String>`, and `String` already
/// implements `Display`, which is exactly what cxx's `Result<T>` sugar
/// requires of an error type (it gets turned into a thrown `rust::Error` on
/// the C++ side via `Display::to_string`) — so no extra error wrapper type
/// is needed here.
fn run_escrow_mocked(wasm: &[u8], gas: u64) -> Result<ffi::RunResult, String> {
    let host = SampleHost;
    let out = run_escrow(wasm, gas, &host, "finish")?;
    Ok(ffi::RunResult {
        result: out.result,
        fuel_used: out.fuel_used,
    })
}

/// Compiles WebAssembly text (WAT) to wasm bytes. A tooling/test helper so
/// callers (C++ tests, in particular) can express modules as readable text
/// instead of raw byte arrays.
fn compile_wat(wat: &str) -> Result<Vec<u8>, String> {
    wat::parse_str(wat).map_err(|e| format!("wat: {e}"))
}

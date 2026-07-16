//! cxx bridge for the wasm engine. Step 3 (C++ -> Rust coarse entry) is done
//! via `run_escrow_mocked`. Step 4 (Rust -> C++ host-function forwarding) is
//! done via `run_escrow_with_cxx_host`, which services the guest's
//! `sha512_half` import by forwarding across cxx to the existing C++
//! `sha512Half` primitive through a C++-owned `HostContext`.
//!
//! `run_escrow_mocked` still runs a wasm blob's `finish` export against a
//! built-in [`SampleHost`] (no C++ involved) — useful for engine-only tests.
#[cxx::bridge(namespace = "rs::wasm_vm")]
mod bridge {
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

        // Run `wasm`'s `finish` export with `gas` fuel against a C++-backed
        // `HostContext`, proving the Rust->C++ host-function callback path.
        fn run_escrow_with_cxx_host(host: &HostContext, wasm: &[u8], gas: u64)
        -> Result<RunResult>;

        // Compile WebAssembly text (WAT) to wasm bytes. A tooling/test helper
        // so callers can express modules as readable text instead of raw bytes.
        fn compile_wat(wat: &str) -> Result<Vec<u8>>;
    }

    unsafe extern "C++" {
        include!("xrpl/tx/wasm-rs/HostContext.h");

        #[namespace = "xrpl::wasmrs"]
        type HostContext;

        // Each value-producing host call hands C++ `out`, a mutable slice
        // aliasing the guest's output region in wasm linear memory; C++ writes
        // the value's bytes straight into it (the single copy) and returns the
        // value's true length (>= 0), or a negative `HostError` code. There is
        // no owned result struct to marshal across cxx.
        #[namespace = "xrpl::wasmrs"]
        fn sha512_half(self: &HostContext, data: &[u8], out: &mut [u8]) -> i32;

        #[namespace = "xrpl::wasmrs"]
        fn get_ledger_sqn(self: &HostContext, out: &mut [u8]) -> i32;

        #[namespace = "xrpl::wasmrs"]
        fn get_current_ledger_obj_field(self: &HostContext, field: i32, out: &mut [u8]) -> i32;

        #[namespace = "xrpl::wasmrs"]
        fn trace(self: &HostContext, msg: &str, data: &[u8], as_hex: bool) -> i32;

        #[namespace = "xrpl::wasmrs"]
        fn trace_num(self: &HostContext, msg: &str, number: i64) -> i32;
    }
}

use crate::run_escrow;
use host_functions::{HostError, HostFunctions, HostResult};

/// Minimal synthetic-ledger host with no external deps (no `sha2`). Used by
/// `run_escrow_mocked` for engine-only tests; [`CxxHost`] below is the
/// production-shaped host that forwards to C++.
struct SampleHost;

impl HostFunctions for SampleHost {
    fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> {
        let bytes = 7u32.to_le_bytes();
        if bytes.len() <= out.len() {
            out[..bytes.len()].copy_from_slice(&bytes);
        }
        Ok(bytes.len())
    }

    fn get_current_ledger_obj_field(&self, _field: i32, _out: &mut [u8]) -> HostResult<usize> {
        Err(HostError::FieldNotFound)
    }

    fn sha512_half(&self, _data: &[u8], _out: &mut [u8]) -> HostResult<usize> {
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
fn run_escrow_mocked(wasm: &[u8], gas: u64) -> Result<bridge::RunResult, String> {
    let host = SampleHost;
    let out = run_escrow(wasm, gas, &host, "finish")?;
    Ok(bridge::RunResult {
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

/// Sized wrapper that carries the `HostFunctions` impl and forwards to the
/// borrowed C++ `HostContext` (opaque cxx types are `!Sized`, so the trait
/// object needs this wrapper).
struct CxxHost<'a> {
    ctx: &'a bridge::HostContext,
}

impl HostFunctions for CxxHost<'_> {
    fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> {
        // C++ writes the serialized sequence number straight into `out` (guest
        // linear memory) and returns its true length, or a negative `HostError`
        // code.
        let n = self.ctx.get_ledger_sqn(out);
        if n < 0 {
            Err(HostError::from_code(n))
        } else {
            Ok(n as usize)
        }
    }

    fn get_current_ledger_obj_field(&self, field: i32, out: &mut [u8]) -> HostResult<usize> {
        // C++ writes the field's bytes straight into `out` and returns the
        // field's true length, or a negative `HostError` code.
        let n = self.ctx.get_current_ledger_obj_field(field, out);
        if n < 0 {
            Err(HostError::from_code(n))
        } else {
            Ok(n as usize)
        }
    }

    fn sha512_half(&self, data: &[u8], out: &mut [u8]) -> HostResult<usize> {
        // C++ writes the digest straight into `out` and returns its true length
        // (32), or a negative `HostError` code.
        let n = self.ctx.sha512_half(data, out);
        if n < 0 {
            Err(HostError::from_code(n))
        } else {
            Ok(n as usize)
        }
    }

    fn trace(&self, msg: &str, data: &[u8], as_hex: bool) -> HostResult<()> {
        let s = self.ctx.trace(msg, data, as_hex);
        if s < 0 {
            Err(HostError::from_code(s))
        } else {
            Ok(())
        }
    }

    fn trace_num(&self, msg: &str, number: i64) -> HostResult<()> {
        let s = self.ctx.trace_num(msg, number);
        if s < 0 {
            Err(HostError::from_code(s))
        } else {
            Ok(())
        }
    }
}

/// Runs `wasm`'s `finish` export with `gas` fuel against a C++-backed
/// [`ffi::HostContext`], proving the Rust->C++ host-function callback path:
/// the guest's `sha512_half` import is serviced by forwarding across cxx to
/// the existing C++ `sha512Half` primitive.
fn run_escrow_with_cxx_host(
    host: &bridge::HostContext,
    wasm: &[u8],
    gas: u64,
) -> Result<bridge::RunResult, String> {
    let cxx_host = CxxHost { ctx: host };
    let out = run_escrow(wasm, gas, &cxx_host, "finish")?;
    Ok(bridge::RunResult {
        result: out.result,
        fuel_used: out.fuel_used,
    })
}

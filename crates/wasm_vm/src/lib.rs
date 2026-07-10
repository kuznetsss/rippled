//! The WASM engine driver for programmable escrows.
//!
//! This crate owns the wasmi interpreter. Per the redesign, it:
//!
//! * builds a per-invocation [`Store`] whose data is a [`VmState`] holding a
//!   `Box<dyn HostFunctions>` — so *where* host calls are serviced (mock,
//!   forward-to-C++, …) is a runtime choice the engine never sees;
//! * registers each guest import as a native Rust closure that reads
//!   *bounds-checked* slices out of guest memory, calls the
//!   [`host_functions`] trait, and writes results back — the manual pointer
//!   math from `HostFuncWrapper.cpp` collapses into a few slice helpers here;
//! * meters gas as wasmi fuel and charges each host call, so gas accounting
//!   lives in the engine rather than the C++ call path.
//!
//! Step 2 wires this up against a native `MockHost` (see the tests). The cxx
//! bridges to C++ come in later steps.

use host_functions::{HostError, HostFunctions};
use wasmi::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};

mod ffi;

/// Import module namespace the guest imports host functions from
/// (`(import "host" "ldgr_index" ...)`).
const HOST_MODULE: &str = "host";

/// Per-call gas cost for each host function, mirroring the values registered in
/// `src/libxrpl/tx/wasm/WasmVM.cpp`. Keeping them here is the concrete form of
/// "gas accounting moves into the Rust engine".
mod gas {
    pub const GET_LEDGER_SQN: u64 = 60;
    pub const GET_CURRENT_LEDGER_OBJ_FIELD: u64 = 70;
    pub const SHA512_HALF: u64 = 2_000;
    pub const TRACE: u64 = 500;
    pub const TRACE_NUM: u64 = 500;
}

/// State threaded through every host call, stored in the wasmi [`Store`].
pub struct VmState {
    host: Box<dyn HostFunctions>,
}

/// Outcome of running an escrow contract to completion.
pub struct RunOutcome {
    /// The value returned by the exported entry point (`finish`): `> 0` means
    /// allow the escrow to finish.
    pub result: i32,
    /// Fuel (gas) consumed by the whole invocation — guest instructions plus
    /// the per-call host charges.
    pub fuel_used: u64,
}

/// Build the wasmi engine with the sandboxing knobs the escrow VM requires.
/// (Unchanged from the original skeleton: a deterministic, minimal-feature
/// configuration with fuel metering on.)
pub fn build_wasm_engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    config.ignore_custom_sections(true);
    config.wasm_mutable_global(false);
    config.wasm_multi_value(false);
    config.wasm_sign_extension(false);
    config.wasm_saturating_float_to_int(false);
    config.wasm_bulk_memory(false);
    config.wasm_reference_types(false);
    config.wasm_tail_call(false);
    config.wasm_extended_const(false);
    config.floats(false);
    config.wasm_multi_memory(false);
    config.wasm_custom_page_sizes(false);
    config.wasm_memory64(false);
    config.wasm_wide_arithmetic(false);
    Engine::new(&config)
}

/// Run an escrow contract: compile `wasm`, give it `gas` fuel, service its host
/// calls through `host`, and call the exported `function_name` (`finish`).
///
/// This is the coarse, once-per-finish entry the C++ side will call across cxx
/// in Step 3.
pub fn run_escrow(
    wasm: &[u8],
    gas: u64,
    host: Box<dyn HostFunctions>,
    function_name: &str,
) -> Result<RunOutcome, String> {
    let engine = build_wasm_engine();
    let module = Module::new(&engine, wasm).map_err(|e| format!("compile: {e}"))?;

    let mut store = Store::new(&engine, VmState { host });
    store.set_fuel(gas).map_err(|e| format!("set_fuel: {e}"))?;

    let mut linker = Linker::<VmState>::new(&engine);
    register_host_functions(&mut linker)?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| format!("instantiate: {e}"))?;
    let finish = instance
        .get_typed_func::<(), i32>(&store, function_name)
        .map_err(|e| format!("no entry point '{function_name}': {e}"))?;

    let result = finish
        .call(&mut store, ())
        .map_err(|e| format!("trap: {e}"))?;

    let remaining = store.get_fuel().unwrap_or(0);
    Ok(RunOutcome {
        result,
        fuel_used: gas.saturating_sub(remaining),
    })
}

// ---------------------------------------------------------------------------
// Import registration
// ---------------------------------------------------------------------------

/// Register the PoC's host functions on `linker`. Each closure only adapts wasm
/// i32/i64 arguments to a typed `hf_*` helper below; the helpers hold the real
/// logic and are easy to read and (later) test in isolation.
fn register_host_functions(linker: &mut Linker<VmState>) -> Result<(), String> {
    let link_err = |e: wasmi::errors::LinkerError| format!("register import: {e}");

    linker
        .func_wrap(HOST_MODULE, "ldgr_index", |mut caller: Caller<'_, VmState>| -> i64 {
            ret64(hf_get_ledger_sqn(&mut caller))
        })
        .map_err(link_err)?;

    linker
        .func_wrap(
            HOST_MODULE,
            "home_le_field",
            |mut caller: Caller<'_, VmState>, field: i32, out_ptr: i32, out_len: i32| -> i32 {
                ret(hf_get_current_ledger_obj_field(&mut caller, field, out_ptr, out_len))
            },
        )
        .map_err(link_err)?;

    linker
        .func_wrap(
            HOST_MODULE,
            "sha512_half",
            |mut caller: Caller<'_, VmState>, dp: i32, dl: i32, op: i32, ol: i32| -> i32 {
                ret(hf_sha512_half(&mut caller, dp, dl, op, ol))
            },
        )
        .map_err(link_err)?;

    linker
        .func_wrap(
            HOST_MODULE,
            "trace",
            |mut caller: Caller<'_, VmState>, mp: i32, ml: i32, dp: i32, dl: i32, hex: i32| -> i32 {
                ret(hf_trace(&mut caller, mp, ml, dp, dl, hex))
            },
        )
        .map_err(link_err)?;

    linker
        .func_wrap(
            HOST_MODULE,
            "trace_num",
            |mut caller: Caller<'_, VmState>, mp: i32, ml: i32, n: i64| -> i32 {
                ret(hf_trace_num(&mut caller, mp, ml, n))
            },
        )
        .map_err(link_err)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Host-function bodies (typed; memory marshaling lives in the slice helpers)
// ---------------------------------------------------------------------------

fn hf_get_ledger_sqn(caller: &mut Caller<'_, VmState>) -> Result<i64, HostError> {
    charge(caller, gas::GET_LEDGER_SQN)?;
    Ok(caller.data().host.get_ledger_sqn()? as i64)
}

fn hf_get_current_ledger_obj_field(
    caller: &mut Caller<'_, VmState>,
    field_code: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, HostError> {
    charge(caller, gas::GET_CURRENT_LEDGER_OBJ_FIELD)?;
    let mem = memory(caller)?;
    let bytes = caller.data().host.get_current_ledger_obj_field(field_code)?;
    write_bytes(caller, &mem, out_ptr, out_len, &bytes)
}

fn hf_sha512_half(
    caller: &mut Caller<'_, VmState>,
    data_ptr: i32,
    data_len: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, HostError> {
    charge(caller, gas::SHA512_HALF)?;
    let mem = memory(caller)?;
    let input = read_bytes(caller, &mem, data_ptr, data_len)?;
    let digest = caller.data().host.sha512_half(&input)?;
    write_bytes(caller, &mem, out_ptr, out_len, &digest)
}

fn hf_trace(
    caller: &mut Caller<'_, VmState>,
    msg_ptr: i32,
    msg_len: i32,
    data_ptr: i32,
    data_len: i32,
    as_hex: i32,
) -> Result<i32, HostError> {
    charge(caller, gas::TRACE)?;
    let mem = memory(caller)?;
    let msg = read_str(caller, &mem, msg_ptr, msg_len)?;
    let data = read_bytes(caller, &mem, data_ptr, data_len)?;
    caller.data().host.trace(&msg, &data, as_hex != 0)?;
    Ok(0)
}

fn hf_trace_num(
    caller: &mut Caller<'_, VmState>,
    msg_ptr: i32,
    msg_len: i32,
    number: i64,
) -> Result<i32, HostError> {
    charge(caller, gas::TRACE_NUM)?;
    let mem = memory(caller)?;
    let msg = read_str(caller, &mem, msg_ptr, msg_len)?;
    caller.data().host.trace_num(&msg, number)?;
    Ok(0)
}

// ---------------------------------------------------------------------------
// Gas + bounds-checked memory helpers (the crate's only "unsafe surface",
// concentrated and safe: every access is a checked wasmi slice op)
// ---------------------------------------------------------------------------

/// Deduct `cost` fuel for a host call; `OutOfGas` if it would go negative.
fn charge<T>(caller: &mut Caller<'_, T>, cost: u64) -> Result<(), HostError> {
    let remaining = caller.get_fuel().map_err(|_| HostError::Internal)?;
    match remaining.checked_sub(cost) {
        Some(left) => caller.set_fuel(left).map_err(|_| HostError::Internal),
        None => {
            let _ = caller.set_fuel(0);
            Err(HostError::OutOfGas)
        }
    }
}

/// The guest's exported linear memory.
fn memory<T>(caller: &Caller<'_, T>) -> Result<Memory, HostError> {
    match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => Ok(mem),
        _ => Err(HostError::NoMemExported),
    }
}

/// Copy `len` bytes out of guest memory at `ptr`.
fn read_bytes<T>(
    caller: &Caller<'_, T>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, HostError> {
    if ptr < 0 || len < 0 {
        return Err(HostError::InvalidParams);
    }
    let mut buf = vec![0u8; len as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .map_err(|_| HostError::PointerOutOfBounds)?;
    Ok(buf)
}

/// Copy a UTF-8 string out of guest memory at `ptr`.
fn read_str<T>(
    caller: &Caller<'_, T>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> Result<String, HostError> {
    let bytes = read_bytes(caller, mem, ptr, len)?;
    String::from_utf8(bytes).map_err(|_| HostError::Decoding)
}

/// Write `src` into the guest buffer `[dst, dst + cap)`; returns bytes written,
/// or `BufferTooSmall` if the guest's buffer can't hold it.
fn write_bytes<T>(
    caller: &mut Caller<'_, T>,
    mem: &Memory,
    dst: i32,
    cap: i32,
    src: &[u8],
) -> Result<i32, HostError> {
    if dst < 0 || cap < 0 {
        return Err(HostError::InvalidParams);
    }
    if src.len() > cap as usize {
        return Err(HostError::BufferTooSmall);
    }
    mem.write(&mut *caller, dst as usize, src)
        .map_err(|_| HostError::PointerOutOfBounds)?;
    Ok(src.len() as i32)
}

/// Collapse a host result to the guest-visible `i32` (negative = error code).
fn ret(r: Result<i32, HostError>) -> i32 {
    r.unwrap_or_else(|e| e.code())
}

/// Same, for the one scalar function that returns `i64`.
fn ret64(r: Result<i64, HostError>) -> i64 {
    r.unwrap_or_else(|e| e.code() as i64)
}

// ---------------------------------------------------------------------------
// Tests: a native MockHost implementing the trait + an end-to-end run of a
// hand-written wasm guest. No C++ involved — proves the engine + trait.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use host_functions::{HostResult, HASH_LEN};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records the trace calls a run makes, so the test can inspect them after
    /// the host `Box` is dropped inside `run_escrow`.
    #[derive(Default)]
    struct Recording {
        traces: RefCell<Vec<String>>,
        nums: RefCell<Vec<(String, i64)>>,
    }

    /// A synthetic-ledger implementation of the host ABI — the "simulator"
    /// flavor. The whole PoC runs against this with no C++ linked.
    struct MockHost {
        ledger_sqn: u32,
        rec: Rc<Recording>,
    }

    impl HostFunctions for MockHost {
        fn get_ledger_sqn(&self) -> HostResult<u32> {
            Ok(self.ledger_sqn)
        }

        fn get_current_ledger_obj_field(&self, field_code: i32) -> HostResult<Vec<u8>> {
            // Canned "escrow object": field 1 -> three bytes, everything else absent.
            match field_code {
                1 => Ok(vec![0xAA, 0xBB, 0xCC]),
                _ => Err(HostError::FieldNotFound),
            }
        }

        fn sha512_half(&self, data: &[u8]) -> HostResult<[u8; HASH_LEN]> {
            use sha2::{Digest, Sha512};
            let full = Sha512::digest(data);
            let mut out = [0u8; HASH_LEN];
            out.copy_from_slice(&full[..HASH_LEN]);
            Ok(out)
        }

        fn trace(&self, msg: &str, _data: &[u8], _as_hex: bool) -> HostResult<()> {
            self.rec.traces.borrow_mut().push(msg.to_string());
            Ok(())
        }

        fn trace_num(&self, msg: &str, number: i64) -> HostResult<()> {
            self.rec.nums.borrow_mut().push((msg.to_string(), number));
            Ok(())
        }
    }

    fn sha512_half_bytes(data: &[u8]) -> [u8; HASH_LEN] {
        use sha2::{Digest, Sha512};
        let full = Sha512::digest(data);
        let mut out = [0u8; HASH_LEN];
        out.copy_from_slice(&full[..HASH_LEN]);
        out
    }

    #[test]
    fn runs_guest_and_services_host_calls() {
        // Guest: read the ledger sqn, trace it, sha512Half("hello") into memory
        // at offset 64, and return the first byte of that digest.
        let wat = r#"
            (module
              (import "host" "ldgr_index"  (func $ldgr_index (result i64)))
              (import "host" "sha512_half" (func $sha512_half (param i32 i32 i32 i32) (result i32)))
              (import "host" "trace_num"   (func $trace_num (param i32 i32 i64) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0)  "hello")
              (data (i32.const 16) "sqn")
              (func (export "finish") (result i32)
                (local $sqn i64)
                (local.set $sqn (call $ldgr_index))
                (drop (call $trace_num (i32.const 16) (i32.const 3) (local.get $sqn)))
                (drop (call $sha512_half (i32.const 0) (i32.const 5) (i32.const 64) (i32.const 32)))
                (i32.load8_u (i32.const 64)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");

        let rec = Rc::new(Recording::default());
        let host = Box::new(MockHost {
            ledger_sqn: 42,
            rec: rec.clone(),
        });

        let outcome = run_escrow(&wasm, 1_000_000, host, "finish").expect("run ok");

        // finish returned the first byte of sha512Half("hello").
        let expected = sha512_half_bytes(b"hello");
        assert_eq!(outcome.result, expected[0] as i32);

        // The guest passed the ledger sqn it read straight into trace_num.
        assert_eq!(rec.nums.borrow().as_slice(), &[("sqn".to_string(), 42i64)]);

        // Gas was metered.
        assert!(outcome.fuel_used > 0, "fuel should have been consumed");
    }

    #[test]
    fn buffer_too_small_is_reported_to_guest() {
        // Ask sha512_half to write 32 bytes into a 10-byte buffer.
        let wat = r#"
            (module
              (import "host" "sha512_half" (func $sha512_half (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "hello")
              (func (export "finish") (result i32)
                (call $sha512_half (i32.const 0) (i32.const 5) (i32.const 64) (i32.const 10)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");

        let host = Box::new(MockHost {
            ledger_sqn: 1,
            rec: Rc::new(Recording::default()),
        });
        let outcome = run_escrow(&wasm, 1_000_000, host, "finish").expect("run ok");

        assert_eq!(outcome.result, HostError::BufferTooSmall.code());
    }
}

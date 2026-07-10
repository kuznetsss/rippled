//! The WASM engine driver for programmable escrows.
//!
//! This crate owns the wasmi interpreter. Per the redesign, it:
//!
//! * builds a per-invocation [`Store`] whose data is a [`VmState`] holding a
//!   `&dyn HostFunctions` — so *where* host calls are serviced (mock,
//!   forward-to-C++, …) is a runtime choice the engine never sees, and a host
//!   may borrow caller-owned state for the duration of the call;
//! * registers each guest import as a native Rust closure that reads
//!   *bounds-checked* slices out of guest memory, calls the
//!   [`host_functions`] trait, and writes results back — the manual pointer
//!   math from `HostFuncWrapper.cpp` collapses into a few slice helpers here;
//! * meters gas as wasmi fuel and charges each host call, so gas accounting
//!   lives in the engine rather than the C++ call path.
//!
//! Step 2 wires this up against a native `MockHost` (see the tests). The cxx
//! bridges to C++ come in later steps.

mod abi;
mod ffi;
mod imports;
mod vm;

pub use vm::{build_wasm_engine, run_escrow, RunOutcome, VmState};

// ---------------------------------------------------------------------------
// Tests: a native MockHost implementing the trait + an end-to-end run of a
// hand-written wasm guest. No C++ involved — proves the engine + trait.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::run_escrow;
    use host_functions::{HostError, HostFunctions, HostResult, HASH_LEN};
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
        let host = MockHost {
            ledger_sqn: 42,
            rec: rec.clone(),
        };

        let outcome = run_escrow(&wasm, 1_000_000, &host, "finish").expect("run ok");

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

        let host = MockHost {
            ledger_sqn: 1,
            rec: Rc::new(Recording::default()),
        };
        let outcome = run_escrow(&wasm, 1_000_000, &host, "finish").expect("run ok");

        assert_eq!(outcome.result, HostError::BufferTooSmall.code());
    }

    #[test]
    fn host_may_borrow_local_state() {
        use std::cell::RefCell;

        /// A host that borrows a caller-owned, non-`'static` `RefCell` — the
        /// scenario this refactor exists for (e.g. a future `CxxHost` borrowing
        /// a C++ context for the duration of one call).
        struct BorrowingHost<'a> {
            seen: &'a RefCell<Vec<i64>>,
        }

        impl HostFunctions for BorrowingHost<'_> {
            fn get_ledger_sqn(&self) -> HostResult<u32> {
                Ok(9)
            }

            fn get_current_ledger_obj_field(&self, _field_code: i32) -> HostResult<Vec<u8>> {
                Err(HostError::FieldNotFound)
            }

            fn sha512_half(&self, _data: &[u8]) -> HostResult<[u8; HASH_LEN]> {
                Err(HostError::Internal)
            }

            fn trace(&self, _msg: &str, _data: &[u8], _as_hex: bool) -> HostResult<()> {
                Ok(())
            }

            fn trace_num(&self, _msg: &str, number: i64) -> HostResult<()> {
                self.seen.borrow_mut().push(number);
                Ok(())
            }
        }

        // Guest: read the ledger sqn, pass it to trace_num, and return it.
        let wat = r#"
            (module
              (import "host" "ldgr_index" (func $ldgr_index (result i64)))
              (import "host" "trace_num"  (func $trace_num (param i32 i32 i64) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "sqn")
              (func (export "finish") (result i32)
                (local $s i64)
                (local.set $s (call $ldgr_index))
                (drop (call $trace_num (i32.const 0) (i32.const 3) (local.get $s)))
                (i32.wrap_i64 (local.get $s))))
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");

        let seen = RefCell::new(Vec::new());
        let host = BorrowingHost { seen: &seen };

        let out = run_escrow(&wasm, 1_000_000, &host, "finish").expect("run ok");

        assert_eq!(out.result, 9);
        assert_eq!(*seen.borrow(), vec![9i64]);
    }
}

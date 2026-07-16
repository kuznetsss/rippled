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

pub use vm::{run_escrow, wasm_engine, RunOutcome, VmState};

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
        fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> {
            let bytes = self.ledger_sqn.to_le_bytes();
            if bytes.len() <= out.len() {
                out[..bytes.len()].copy_from_slice(&bytes);
            }
            Ok(bytes.len())
        }

        fn get_current_ledger_obj_field(
            &self,
            field_code: i32,
            out: &mut [u8],
        ) -> HostResult<usize> {
            // Canned "escrow object": field 1 -> three bytes; field 3 -> one
            // byte over the 1 KiB field cap (`MAX_WASM_DATA_LEN`), to drive
            // the `DataFieldTooLarge` guardrail; field 4 -> exactly 1 KiB, to
            // drive the transfer-limit / field-cap boundary tests; everything
            // else absent.
            //
            // Fill-buffer contract: write into `out` only when the value fits,
            // and always return its *true* length — the engine enforces the
            // cap / buffer-fit / transfer policy from that length.
            let data: &[u8] = match field_code {
                1 => &[0xAA, 0xBB, 0xCC],
                3 => &[0u8; 1025],
                4 => &[0u8; 1024],
                _ => return Err(HostError::FieldNotFound),
            };
            if data.len() <= out.len() {
                out[..data.len()].copy_from_slice(data);
            }
            Ok(data.len())
        }

        fn sha512_half(&self, data: &[u8], out: &mut [u8]) -> HostResult<usize> {
            use sha2::{Digest, Sha512};
            let full = Sha512::digest(data);
            let digest = &full[..HASH_LEN];
            if digest.len() <= out.len() {
                out[..digest.len()].copy_from_slice(digest);
            }
            Ok(digest.len())
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
              (import "host" "ldgr_index"  (func $ldgr_index (param i32 i32) (result i32)))
              (import "host" "sha512_half" (func $sha512_half (param i32 i32 i32 i32) (result i32)))
              (import "host" "trace_num"   (func $trace_num (param i32 i32 i64) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0)  "hello")
              (data (i32.const 16) "sqn")
              (func (export "finish") (result i32)
                (local $sqn i64)
                (drop (call $ldgr_index (i32.const 32) (i32.const 4)))
                (local.set $sqn (i64.extend_i32_u (i32.load (i32.const 32))))
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
            fn get_ledger_sqn(&self, out: &mut [u8]) -> HostResult<usize> {
                let bytes = 9u32.to_le_bytes();
                if bytes.len() <= out.len() {
                    out[..bytes.len()].copy_from_slice(&bytes);
                }
                Ok(bytes.len())
            }

            fn get_current_ledger_obj_field(
                &self,
                _field_code: i32,
                _out: &mut [u8],
            ) -> HostResult<usize> {
                Err(HostError::FieldNotFound)
            }

            fn sha512_half(&self, _data: &[u8], _out: &mut [u8]) -> HostResult<usize> {
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
              (import "host" "ldgr_index" (func $ldgr_index (param i32 i32) (result i32)))
              (import "host" "trace_num"  (func $trace_num (param i32 i32 i64) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "sqn")
              (func (export "finish") (result i32)
                (local $s i64)
                (drop (call $ldgr_index (i32.const 16) (i32.const 4)))
                (local.set $s (i64.extend_i32_u (i32.load (i32.const 16))))
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

    /// Closes the loop end to end: a real `example_contract` smart contract,
    /// written against `stdlib`'s `HostFunctions` implementation and compiled
    /// to wasm32-unknown-unknown, run through this engine with its host calls
    /// serviced by the same `MockHost` the hand-written-WAT tests above use.
    #[test]
    fn runs_a_compiled_wasm32_contract() {
        let wasm = include_bytes!("../tests/fixtures/example_contract.wasm");
        let rec = Rc::new(Recording::default());
        let host = MockHost {
            ledger_sqn: 42,
            rec: rec.clone(),
        };
        let out = run_escrow(wasm, 100_000_000, &host, "finish").expect("contract runs");
        assert_eq!(out.result, 1); // 42 >= 10 -> allow finish
        assert_eq!(
            rec.nums.borrow().as_slice(),
            &[("ledger_sqn".to_string(), 42i64)]
        );
        assert!(out.fuel_used > 0);
    }

    // -----------------------------------------------------------------------
    // Guardrail tests: linear-memory page cap, per-run transfer limit, and
    // the 1 KiB per-field size cap.
    // -----------------------------------------------------------------------

    fn fresh_host() -> MockHost {
        MockHost {
            ledger_sqn: 1,
            rec: Rc::new(Recording::default()),
        }
    }

    /// 128 pages (`crate::vm::MAX_MEMORY_PAGES`) is the cap; landing exactly
    /// on it via `memory.grow` must still succeed.
    #[test]
    fn memory_grow_to_exactly_the_cap_succeeds() {
        // 127 initial pages + growing by 1 lands exactly on the 128-page cap.
        let wat = r#"
            (module
              (memory (export "memory") 127)
              (func (export "finish") (result i32)
                (memory.grow (i32.const 1)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, 127); // memory.grow returns the previous size on success
    }

    /// One page past the cap must fail: `run_escrow`'s `trap_on_grow_failure`
    /// setting means this surfaces as a trap, not a `-1` guest-visible return.
    #[test]
    fn memory_grow_one_page_past_the_cap_traps() {
        // Same starting point as above, but grow by 2: lands one page past
        // the 128-page cap.
        let wat = r#"
            (module
              (memory (export "memory") 127)
              (func (export "finish") (result i32)
                (memory.grow (i32.const 2)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let err = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish")
            .expect_err("growth past the cap should trap");
        assert!(err.contains("trap"), "unexpected error: {err}");
    }

    /// Declaring initial memory beyond the cap must fail at instantiation,
    /// before the guest ever runs (mirrors wasmi's own `ResourceLimiter`
    /// instantiation-time behavior).
    #[test]
    fn initial_memory_beyond_cap_fails_to_instantiate() {
        // 200 initial pages (12.5 MiB) exceeds the 128-page (8 MiB) cap.
        let wat = r#"
            (module
              (memory (export "memory") 200)
              (func (export "finish") (result i32) (i32.const 0))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let err = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish")
            .expect_err("oversized initial memory should fail to instantiate");
        assert!(err.contains("instantiate"), "unexpected error: {err}");
    }

    /// A field read at exactly 1024 bytes (`MAX_WASM_DATA_LEN`) is allowed.
    #[test]
    fn field_read_at_exactly_the_cap_succeeds() {
        let wat = r#"
            (module
              (import "host" "sha512_half" (func $sha512_half (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "finish") (result i32)
                (call $sha512_half (i32.const 0) (i32.const 1024) (i32.const 2048) (i32.const 32)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, 32); // wrote the full 32-byte digest
    }

    /// A field read one byte over 1024 (`MAX_WASM_DATA_LEN`) is rejected with
    /// `DataFieldTooLarge` — and the existing-smell fix means this happens
    /// before any `len`-sized allocation.
    #[test]
    fn oversized_field_read_is_rejected() {
        let wat = r#"
            (module
              (import "host" "sha512_half" (func $sha512_half (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "finish") (result i32)
                (call $sha512_half (i32.const 0) (i32.const 1025) (i32.const 2048) (i32.const 32)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, HostError::DataFieldTooLarge.code());
    }

    /// A field write at exactly 1024 bytes is allowed (host's field 4 in
    /// `MockHost::get_current_ledger_obj_field` returns exactly the cap).
    #[test]
    fn field_write_at_exactly_the_cap_succeeds() {
        let wat = r#"
            (module
              (import "host" "home_le_field" (func $home_le_field (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "finish") (result i32)
                (call $home_le_field (i32.const 4) (i32.const 0) (i32.const 1024)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, 1024);
    }

    /// A field write one byte over the cap (host's field 3 returns 1025
    /// bytes) is rejected with `DataFieldTooLarge`, before it ever touches
    /// the transfer budget — matching the C++ order (size cap precedes the
    /// transfer check).
    #[test]
    fn oversized_field_write_is_rejected_before_transfer_check() {
        let wat = r#"
            (module
              (import "host" "home_le_field" (func $home_le_field (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "finish") (result i32)
                (call $home_le_field (i32.const 3) (i32.const 0) (i32.const 2000)))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 1_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, HostError::DataFieldTooLarge.code());
    }

    /// Drives cumulative host<->guest byte traffic past the 1 MiB per-run
    /// transfer limit (`crate::vm::TRANSFER_LIMIT_BYTES`) and checks the
    /// guest sees `OutOfTransferLimit` once the budget is exhausted.
    ///
    /// Each loop iteration writes exactly 1024 bytes (host field 4, the
    /// `MAX_WASM_DATA_LEN` cap) via `get_current_ledger_obj_field`, so the
    /// budget is exhausted after exactly 1024 successful calls
    /// (1024 * 1024 == 1 << 20); the 1025th call must fail.
    #[test]
    fn cumulative_transfer_past_the_limit_is_rejected() {
        let wat = r#"
            (module
              (import "host" "home_le_field" (func $home_le_field (param i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "finish") (result i32)
                (local $i i32)
                (local $r i32)
                (block $done
                  (loop $again
                    (local.set $r (call $home_le_field (i32.const 4) (i32.const 0) (i32.const 1024)))
                    (br_if $done (i32.lt_s (local.get $r) (i32.const 0)))
                    (local.set $i (i32.add (local.get $i) (i32.const 1)))
                    ;; safety cap: comfortably more than the ~1024 calls
                    ;; needed to exhaust the 1 MiB budget.
                    (br_if $done (i32.ge_s (local.get $i) (i32.const 2000)))
                    (br $again)
                  )
                )
                (local.get $r))
            )
        "#;
        let wasm = wat::parse_str(wat).expect("valid wat");
        let out = run_escrow(&wasm, 5_000_000, &fresh_host(), "finish").expect("run ok");
        assert_eq!(out.result, HostError::OutOfTransferLimit.code());
    }
}

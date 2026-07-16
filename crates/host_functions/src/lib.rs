//! The host/guest ABI for programmable-escrow WASM contracts.
//!
//! This crate is the *single source of truth* for what a smart contract may ask
//! the host to do. It contains only the [`HostFunctions`] trait, the shared
//! domain types, and the [`HostError`] codes — no engine, no cxx, no ledger.
//!
//! Everything else in the PoC is an *implementation of, or a caller of, this
//! trait*:
//!
//! * the `wasm_vm` engine calls the trait to service guest imports, holding a
//!   `dyn HostFunctions` in the wasmi `Store`;
//! * a `MockHost` implements it against a synthetic ledger (tests / simulator);
//! * a `CxxHost` implements it by forwarding to the existing C++ primitives
//!   (production);
//! * the `stdlib` guest crate implements it too — there each method *is* a wasm
//!   import a contract calls.
//!
//! Because both sides of the FFI boundary point at this one definition, the
//! signatures, types, and error codes cannot drift apart.
//!
//! `no_std` (with `alloc`) so the identical crate links into the wasm32 guest.

#![no_std]

extern crate alloc;

use host_functions_macros::host_abi;

/// Error codes a host function may return.
///
/// The discriminants mirror `HostFunctionError` in
/// `include/xrpl/tx/wasm/WasmCommon.h`, so a negative `i32` crossing the wasm
/// boundary means the same thing to the guest, the Rust host, and the existing
/// C++ code. The full set is kept (not just the ones the PoC uses today) to
/// preserve that shared meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HostError {
    Internal = -1,
    FieldNotFound = -2,
    BufferTooSmall = -3,
    NoArray = -4,
    NotLeafField = -5,
    LocatorMalformed = -6,
    SlotOutRange = -7,
    SlotsFull = -8,
    EmptySlot = -9,
    LedgerObjNotFound = -10,
    Decoding = -11,
    DataFieldTooLarge = -12,
    PointerOutOfBounds = -13,
    NoMemExported = -14,
    InvalidParams = -15,
    InvalidAccount = -16,
    InvalidField = -17,
    IndexOutOfBounds = -18,
    FloatInputMalformed = -19,
    FloatComputationError = -20,
    NoRuntime = -21,
    OutOfGas = -22,
    OutOfTransferLimit = -23,
}

impl HostError {
    /// The negative wire value the guest sees as the function's return code.
    #[inline]
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// Reconstruct a `HostError` from its wire code; unknown/positive values map to `Internal`.
    pub const fn from_code(code: i32) -> HostError {
        match code {
            -1 => HostError::Internal,
            -2 => HostError::FieldNotFound,
            -3 => HostError::BufferTooSmall,
            -4 => HostError::NoArray,
            -5 => HostError::NotLeafField,
            -6 => HostError::LocatorMalformed,
            -7 => HostError::SlotOutRange,
            -8 => HostError::SlotsFull,
            -9 => HostError::EmptySlot,
            -10 => HostError::LedgerObjNotFound,
            -11 => HostError::Decoding,
            -12 => HostError::DataFieldTooLarge,
            -13 => HostError::PointerOutOfBounds,
            -14 => HostError::NoMemExported,
            -15 => HostError::InvalidParams,
            -16 => HostError::InvalidAccount,
            -17 => HostError::InvalidField,
            -18 => HostError::IndexOutOfBounds,
            -19 => HostError::FloatInputMalformed,
            -20 => HostError::FloatComputationError,
            -21 => HostError::NoRuntime,
            -22 => HostError::OutOfGas,
            -23 => HostError::OutOfTransferLimit,
            _ => HostError::Internal,
        }
    }
}

/// Convenience alias for the trait's fallible returns.
pub type HostResult<T> = Result<T, HostError>;

/// A `sha512Half` digest: the first 32 bytes of a SHA-512, as XRPL uses it.
pub const HASH_LEN: usize = 32;

/// Per-function ABI metadata: the wasm import name and the consensus-fixed base gas cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostFnSpec {
    pub name: &'static str,
    pub base_gas: u64,
}

// The host functions a guest contract may call.
//
// Declared once via `host_abi!` as the single source of truth: each entry
// below carries its wasm import name (`#[wasm]`) and consensus-fixed base gas
// cost (`#[gas]`), and expands to both the `HostFn` enum (metadata, for the
// engine's import registration and gas accounting) and the `HostFunctions`
// trait.
//
// The entries are written in terms of ordinary Rust types (`&[u8]`, `Vec<u8>`,
// `&str`, `u32`, `[u8; N]`). They say nothing about wasm linear memory or
// pointers — that marshaling lives in the engine on the host side and in
// `stdlib` on the guest side. The trait stays object-safe so the engine can
// hold a `Box<dyn HostFunctions>` in its `Store`.
//
// Note the one lowering the macro applies rather than mirroring the declared
// type verbatim: a *value-producing* return (`Vec<u8>` or `[u8; N]`) becomes a
// fill-the-caller's-buffer method — `fn(&self, .., out: &mut [u8]) ->
// HostResult<usize>` (bytes written). The host writes straight into `out`, so
// the engine can hand it a slice aliasing guest linear memory and every such
// host function writes directly into wasm memory with no owned intermediate to
// copy through. The declared `Vec<u8>` / `[u8; N]` documents whether the value
// is variable- or fixed-length; the engine enforces the buffer-fit / field-cap
// / transfer policy from the returned length either way.
//
// The PoC exposes five functions, each chosen to exercise a distinct ABI
// shape:
// * `get_ledger_sqn` — fill a caller buffer with a fixed 4-byte little-endian
//   serialized sequence number, ledger read (needs host context);
// * `get_current_ledger_obj_field` — read a scalar in, fill a caller buffer
//   with a variable-length value;
// * `sha512_half` — read a byte slice, fill a caller buffer with a fixed-size
//   digest (a pure function, later forwarded to C++);
// * `trace` / `trace_num` — read a byte slice in, return nothing (debug).
host_abi! {
    /// Sequence number of the current ledger, as its 4 little-endian bytes.
    #[gas = 60]
    #[wasm = "ldgr_index"]
    fn get_ledger_sqn() -> [u8; 4];

    /// Serialized bytes of a field on the current (escrow) ledger object.
    #[gas = 70]
    #[wasm = "home_le_field"]
    fn get_current_ledger_obj_field(field: i32) -> Vec<u8>;

    /// The XRPL sha512Half (first 32 bytes of SHA-512) of `data`.
    #[gas = 2000]
    #[wasm = "sha512_half"]
    fn sha512_half(data: &[u8]) -> [u8; 32];

    /// Emit a trace line with a byte payload.
    #[gas = 500]
    #[wasm = "trace"]
    fn trace(msg: &str, data: &[u8], as_hex: bool);

    /// Emit a trace line with a signed integer.
    #[gas = 500]
    #[wasm = "trace_num"]
    fn trace_num(msg: &str, number: i64);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial implementation of every `HostFunctions` method, existing only
    /// to prove the macro-generated trait shape compiles and is object-safe.
    struct Dummy;

    impl HostFunctions for Dummy {
        fn get_ledger_sqn(&self, _out: &mut [u8]) -> HostResult<usize> {
            Ok(0)
        }

        fn get_current_ledger_obj_field(&self, _field: i32, _out: &mut [u8]) -> HostResult<usize> {
            Ok(0)
        }

        fn sha512_half(&self, _data: &[u8], _out: &mut [u8]) -> HostResult<usize> {
            Ok(0)
        }

        fn trace(&self, _msg: &str, _data: &[u8], _as_hex: bool) -> HostResult<()> {
            Ok(())
        }

        fn trace_num(&self, _msg: &str, _number: i64) -> HostResult<()> {
            Ok(())
        }
    }

    #[test]
    fn all_lists_every_variant() {
        assert_eq!(HostFn::ALL.len(), 5);
    }

    #[test]
    fn spec_values_are_correct() {
        assert_eq!(HostFn::GetLedgerSqn.spec().name, "ldgr_index");
        assert_eq!(HostFn::GetLedgerSqn.spec().base_gas, 60);

        assert_eq!(
            HostFn::GetCurrentLedgerObjField.spec().name,
            "home_le_field"
        );
        assert_eq!(HostFn::GetCurrentLedgerObjField.spec().base_gas, 70);

        assert_eq!(HostFn::Sha512Half.spec().name, "sha512_half");
        assert_eq!(HostFn::Sha512Half.spec().base_gas, 2000);

        assert_eq!(HostFn::Trace.spec().name, "trace");
        assert_eq!(HostFn::Trace.spec().base_gas, 500);

        assert_eq!(HostFn::TraceNum.spec().name, "trace_num");
        assert_eq!(HostFn::TraceNum.spec().base_gas, 500);
    }

    #[test]
    fn spec_names_are_unique() {
        for (i, a) in HostFn::ALL.iter().enumerate() {
            for b in &HostFn::ALL[i + 1..] {
                assert_ne!(a.spec().name, b.spec().name);
            }
        }
    }

    #[test]
    fn host_functions_trait_is_object_safe() {
        let _: alloc::boxed::Box<dyn HostFunctions> = alloc::boxed::Box::new(Dummy);
    }

    #[test]
    fn from_code_roundtrips_every_variant() {
        let variants = [
            HostError::Internal,
            HostError::FieldNotFound,
            HostError::BufferTooSmall,
            HostError::NoArray,
            HostError::NotLeafField,
            HostError::LocatorMalformed,
            HostError::SlotOutRange,
            HostError::SlotsFull,
            HostError::EmptySlot,
            HostError::LedgerObjNotFound,
            HostError::Decoding,
            HostError::DataFieldTooLarge,
            HostError::PointerOutOfBounds,
            HostError::NoMemExported,
            HostError::InvalidParams,
            HostError::InvalidAccount,
            HostError::InvalidField,
            HostError::IndexOutOfBounds,
            HostError::FloatInputMalformed,
            HostError::FloatComputationError,
            HostError::NoRuntime,
            HostError::OutOfGas,
            HostError::OutOfTransferLimit,
        ];
        for v in variants {
            assert_eq!(HostError::from_code(v.code()), v);
        }
    }

    #[test]
    fn from_code_maps_unknown_to_internal() {
        assert_eq!(HostError::from_code(0), HostError::Internal);
        assert_eq!(HostError::from_code(1), HostError::Internal);
        assert_eq!(HostError::from_code(-999), HostError::Internal);
    }
}

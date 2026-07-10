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

use alloc::vec::Vec;

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
}

/// Convenience alias for the trait's fallible returns.
pub type HostResult<T> = Result<T, HostError>;

/// A `sha512Half` digest: the first 32 bytes of a SHA-512, as XRPL uses it.
pub const HASH_LEN: usize = 32;

/// The host functions a guest contract may call.
///
/// The trait is written in terms of ordinary Rust types (`&[u8]`, `Vec<u8>`,
/// `&str`, `u32`). It says nothing about wasm linear memory or pointers — that
/// marshaling lives in the engine on the host side and in `stdlib` on the guest
/// side. The trait stays object-safe so the engine can hold a
/// `Box<dyn HostFunctions>` in its `Store`.
///
/// The PoC exposes four functions, each chosen to exercise a distinct ABI
/// shape:
/// * [`get_ledger_sqn`](HostFunctions::get_ledger_sqn) — scalar out, ledger
///   read (needs host context);
/// * [`get_current_ledger_obj_field`](HostFunctions::get_current_ledger_obj_field)
///   — read a scalar in, return a variable-length byte buffer;
/// * [`sha512_half`](HostFunctions::sha512_half) — read a byte slice, return a
///   fixed-size buffer (a pure function, later forwarded to C++);
/// * [`trace`](HostFunctions::trace) / [`trace_num`](HostFunctions::trace_num)
///   — read a byte slice in, return nothing (debug).
pub trait HostFunctions {
    /// Sequence number of the ledger the escrow is being finished in.
    fn get_ledger_sqn(&self) -> HostResult<u32>;

    /// Serialized bytes of a field on the current (escrow) ledger object,
    /// selected by its `SField` numeric code.
    fn get_current_ledger_obj_field(&self, field_code: i32) -> HostResult<Vec<u8>>;

    /// The XRPL `sha512Half` (first 32 bytes of SHA-512) of `data`.
    fn sha512_half(&self, data: &[u8]) -> HostResult<[u8; HASH_LEN]>;

    /// Emit a trace line: a UTF-8 message plus a byte payload rendered as hex
    /// (`as_hex`) or raw.
    fn trace(&self, msg: &str, data: &[u8], as_hex: bool) -> HostResult<()>;

    /// Emit a trace line: a UTF-8 message plus a signed integer.
    fn trace_num(&self, msg: &str, number: i64) -> HostResult<()>;
}

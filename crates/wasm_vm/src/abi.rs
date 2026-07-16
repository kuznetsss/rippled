use crate::vm::VmState;
use host_functions::{HostError, HostFn, HostFunctions, HostResult};
use wasmi::{Caller, Extern, Memory};

// ---------------------------------------------------------------------------
// ABI marshaling traits: decode a host-function argument from wasm scalars +
// guest memory (`AbiArg`), encode a result back into guest memory and a wasm
// return status (`AbiRet`), and a single-point gas-charging wrapper
// (`charged`) so every registered closure pays for its call exactly once.
// ---------------------------------------------------------------------------

/// Decode one host-function argument from the wasm scalar(s) the guest passed,
/// reading guest memory for slice/string types. `Raw` is the wasm scalar shape:
/// `i32`/`i64` for a plain scalar, `(i32, i32)` for a (ptr, len) pair.
pub(crate) trait AbiArg: Sized {
    type Raw;
    fn read(caller: &Caller<'_, VmState<'_>>, raw: Self::Raw) -> HostResult<Self>;
}

impl AbiArg for i32 {
    type Raw = i32;
    fn read(_c: &Caller<'_, VmState<'_>>, r: i32) -> HostResult<Self> {
        Ok(r)
    }
}
impl AbiArg for i64 {
    type Raw = i64;
    fn read(_c: &Caller<'_, VmState<'_>>, r: i64) -> HostResult<Self> {
        Ok(r)
    }
}
impl AbiArg for bool {
    type Raw = i32;
    fn read(_c: &Caller<'_, VmState<'_>>, r: i32) -> HostResult<Self> {
        Ok(r != 0)
    }
}

impl AbiArg for Vec<u8> {
    type Raw = (i32, i32);
    fn read(c: &Caller<'_, VmState<'_>>, (ptr, len): (i32, i32)) -> HostResult<Self> {
        let mem = memory(c)?;
        read_bytes(c, &mem, ptr, len)
    }
}

/// Encode a *scalar or unit* host-function result into the status the wasm fn
/// returns (>= 0 success — a value; < 0 a HostError code, via `to_wasm_*`).
/// `Out` is the extra wasm scalars for output — always `()` here, since these
/// returns need no guest buffer.
///
/// Value-producing returns (`Vec<u8>` / `[u8; N]`) do *not* go through this
/// trait: they are serviced by [`write_into`], where the host writes straight
/// into guest linear memory with no owned buffer to encode.
pub(crate) trait AbiRet {
    type Out;
    fn write(self, caller: &mut Caller<'_, VmState<'_>>, out: Self::Out) -> HostResult<i64>;
}

impl AbiRet for () {
    type Out = ();
    fn write(self, _c: &mut Caller<'_, VmState<'_>>, _o: ()) -> HostResult<i64> {
        Ok(0)
    }
}
impl AbiRet for u32 {
    type Out = ();
    fn write(self, _c: &mut Caller<'_, VmState<'_>>, _o: ()) -> HostResult<i64> {
        Ok(self as i64)
    }
}

/// Charge a host call's gas once (from the enum's spec) then run its body.
/// Because every registered closure goes through here, gas can't be forgotten.
pub(crate) fn charged(
    caller: &mut Caller<'_, VmState<'_>>,
    op: HostFn,
    body: impl FnOnce(&mut Caller<'_, VmState<'_>>) -> HostResult<i64>,
) -> HostResult<i64> {
    charge(caller, op.spec().base_gas)?;
    body(caller)
}

pub(crate) fn to_wasm_i32(r: HostResult<i64>) -> i32 {
    match r {
        Ok(v) => v as i32,
        Err(e) => e.code(),
    }
}
#[allow(dead_code)]
pub(crate) fn to_wasm_i64(r: HostResult<i64>) -> i64 {
    match r {
        Ok(v) => v,
        Err(e) => e.code() as i64,
    }
}

// ---------------------------------------------------------------------------
// Gas + bounds-checked memory helpers (the crate's only "unsafe surface",
// concentrated and safe: every access is a checked wasmi slice op)
// ---------------------------------------------------------------------------

/// Per-field size cap for any single value crossing the host/guest boundary.
///
/// Mirrors `kMaxWasmDataLength = 1 * 1024` in
/// `include/xrpl/protocol/Protocol.h:261`, enforced by `getDataSlice`/
/// `setData` (`src/libxrpl/tx/wasm/HostFuncWrapper.cpp`) returning
/// `DataFieldTooLarge`.
const MAX_WASM_DATA_LEN: usize = 1024;

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

/// Deduct `n` bytes from the per-run transfer-limit budget (see
/// [`crate::vm::TRANSFER_LIMIT_BYTES`]); `OutOfTransferLimit` if it would go
/// negative. A separate budget from gas — see `VmState::transfer_budget`.
fn charge_transfer(state: &VmState<'_>, n: usize) -> Result<(), HostError> {
    let n = n as u64;
    let remaining = state.transfer_budget.get();
    match remaining.checked_sub(n) {
        Some(left) => {
            state.transfer_budget.set(left);
            Ok(())
        }
        None => Err(HostError::OutOfTransferLimit),
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
///
/// Checks are ordered to match the C++ host-call sequence: params validity,
/// then the [`MAX_WASM_DATA_LEN`] size cap (`DataFieldTooLarge`), then the
/// transfer-limit budget (`OutOfTransferLimit`) — all *before* allocating the
/// output buffer or touching guest memory, so an oversized/over-budget `len`
/// never drives an allocation.
fn read_bytes(
    caller: &Caller<'_, VmState<'_>>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, HostError> {
    if ptr < 0 || len < 0 {
        return Err(HostError::InvalidParams);
    }
    let len = len as usize;
    if len > MAX_WASM_DATA_LEN {
        return Err(HostError::DataFieldTooLarge);
    }
    charge_transfer(caller.data(), len)?;
    let mut buf = vec![0u8; len];
    mem.read(caller, ptr as usize, &mut buf)
        .map_err(|_| HostError::PointerOutOfBounds)?;
    Ok(buf)
}

/// Bounds-check `[ptr, ptr + len)` and return a `&[u8]` **aliasing guest linear
/// memory** — no allocation, no copy. The read analog of [`write_into`]: where
/// `write_into` hands the host a `&mut [u8]` into guest memory, this hands it a
/// `&[u8]`, so a *read-only* host call touches the guest's bytes in place.
///
/// The returned slice borrows `caller`, so it is valid only for the duration of
/// the host call it feeds — the same leaf-call invariant `write_into` relies on
/// (our host functions don't re-enter the guest and move its memory).
///
/// Checks match [`read_bytes`]: params validity, the [`MAX_WASM_DATA_LEN`] size
/// cap (`DataFieldTooLarge`), then the transfer-limit budget — all before the
/// slice is formed.
pub(crate) fn read_borrowed<'a>(
    caller: &'a Caller<'_, VmState<'_>>,
    ptr: i32,
    len: i32,
) -> HostResult<&'a [u8]> {
    if ptr < 0 || len < 0 {
        return Err(HostError::InvalidParams);
    }
    let (ptr, len) = (ptr as usize, len as usize);
    if len > MAX_WASM_DATA_LEN {
        return Err(HostError::DataFieldTooLarge);
    }
    charge_transfer(caller.data(), len)?;
    let end = ptr.checked_add(len).ok_or(HostError::PointerOutOfBounds)?;
    memory(caller)?
        .data(caller)
        .get(ptr..end)
        .ok_or(HostError::PointerOutOfBounds)
}

/// Service a "fill-the-caller's-buffer" host call: bounds-check the guest
/// output region `[dst, dst + cap)`, hand the host a `&mut [u8]` aliasing it,
/// and let the host write **straight into guest linear memory** — the single
/// copy, with no owned buffer intermediate (this is what removes the extra copy
/// the value-producing host functions used to pay: a `Vec<u8>` / `[u8; N]`
/// materialized on the host side, then copied into guest memory. The `CxxHost`
/// path additionally used to marshal C++ `Bytes` through a `rust::Vec` /
/// `HashResult`; that too is gone).
///
/// `fill` returns the value's *true* length (it writes only when the value fits
/// in `dst`), so the engine keeps ownership of the policy the guest observes:
/// the [`MAX_WASM_DATA_LEN`] field-size cap (`DataFieldTooLarge`), the
/// buffer-fit check (`BufferTooSmall`), and the transfer-limit budget — checked
/// here, in the same order as the C++ `setData` path (size cap precedes the
/// transfer charge). On success returns the byte count.
///
/// Ordering note: because the byte count isn't known until `fill` runs, the
/// transfer budget is charged *after* the write rather than before it (the
/// pre-write gas charge in [`charged`] still bounds how often this runs). A
/// value rejected for being over-cap/over-budget may leave bytes in the guest
/// buffer, but they sit within the guest's own bounds and the guest must treat
/// a negative status as "don't read the buffer".
pub(crate) fn write_into(
    caller: &mut Caller<'_, VmState<'_>>,
    dst: i32,
    cap: i32,
    fill: impl FnOnce(&dyn HostFunctions, &mut [u8]) -> HostResult<usize>,
) -> HostResult<i64> {
    if dst < 0 || cap < 0 {
        return Err(HostError::InvalidParams);
    }
    let (dst, cap) = (dst as usize, cap as usize);
    let mem = memory(caller)?;
    // Copy the shared `&dyn HostFunctions` out of the store data (references are
    // Copy) so the data borrow ends before we borrow guest memory mutably.
    let host: &dyn HostFunctions = caller.data().host;
    let end = dst.checked_add(cap).ok_or(HostError::PointerOutOfBounds)?;
    let out = mem
        .data_mut(&mut *caller)
        .get_mut(dst..end)
        .ok_or(HostError::PointerOutOfBounds)?;

    let n = fill(host, out)?;

    if n > MAX_WASM_DATA_LEN {
        return Err(HostError::DataFieldTooLarge);
    }
    if n > cap {
        return Err(HostError::BufferTooSmall);
    }
    charge_transfer(caller.data(), n)?;
    Ok(n as i64)
}

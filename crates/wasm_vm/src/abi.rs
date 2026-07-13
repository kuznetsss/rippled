use crate::vm::VmState;
use host_functions::{HostError, HostFn, HostResult, HASH_LEN};
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

impl AbiArg for String {
    type Raw = (i32, i32);
    fn read(c: &Caller<'_, VmState<'_>>, (ptr, len): (i32, i32)) -> HostResult<Self> {
        let mem = memory(c)?;
        read_str(c, &mem, ptr, len)
    }
}

/// Encode a host-function result: write bytes into the guest output buffer if
/// the type needs one, and yield the status the wasm fn returns
/// (>= 0 success — a value or byte count; < 0 a HostError code, via `to_wasm_*`).
/// `Out` is the extra wasm scalars for output: `()` for scalar/unit returns,
/// `(i32, i32)` = (out_ptr, out_len) for buffers.
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

impl AbiRet for Vec<u8> {
    type Out = (i32, i32);
    fn write(self, c: &mut Caller<'_, VmState<'_>>, (ptr, cap): (i32, i32)) -> HostResult<i64> {
        let mem = memory(c)?;
        Ok(write_bytes(c, &mem, ptr, cap, &self)? as i64)
    }
}

impl AbiRet for [u8; HASH_LEN] {
    type Out = (i32, i32);
    fn write(self, c: &mut Caller<'_, VmState<'_>>, (ptr, cap): (i32, i32)) -> HostResult<i64> {
        let mem = memory(c)?;
        Ok(write_bytes(c, &mem, ptr, cap, &self)? as i64)
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

/// Copy a UTF-8 string out of guest memory at `ptr`.
fn read_str(
    caller: &Caller<'_, VmState<'_>>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> Result<String, HostError> {
    let bytes = read_bytes(caller, mem, ptr, len)?;
    String::from_utf8(bytes).map_err(|_| HostError::Decoding)
}

/// Write `src` into the guest buffer `[dst, dst + cap)`; returns bytes written.
///
/// Checks, in order: params validity; the [`MAX_WASM_DATA_LEN`] size cap
/// (`DataFieldTooLarge`) — so an oversized field is rejected before it ever
/// touches the transfer budget; `BufferTooSmall` if the guest's buffer can't
/// hold it; then the transfer-limit budget (`OutOfTransferLimit`).
fn write_bytes(
    caller: &mut Caller<'_, VmState<'_>>,
    mem: &Memory,
    dst: i32,
    cap: i32,
    src: &[u8],
) -> Result<i32, HostError> {
    if dst < 0 || cap < 0 {
        return Err(HostError::InvalidParams);
    }
    if src.len() > MAX_WASM_DATA_LEN {
        return Err(HostError::DataFieldTooLarge);
    }
    if src.len() > cap as usize {
        return Err(HostError::BufferTooSmall);
    }
    charge_transfer(caller.data(), src.len())?;
    mem.write(&mut *caller, dst as usize, src)
        .map_err(|_| HostError::PointerOutOfBounds)?;
    Ok(src.len() as i32)
}

use crate::abi::{AbiArg, AbiRet, charged, to_wasm_i32};
use crate::vm::VmState;
use host_functions::{HASH_LEN, HostFn};
use wasmi::{Caller, Linker};

/// Import module namespace the guest imports host functions from
/// (`(import "host" "ldgr_index" ...)`).
const HOST_MODULE: &str = "host";

// ---------------------------------------------------------------------------
// Import registration
// ---------------------------------------------------------------------------

/// Register the PoC's host functions on `linker`, one per [`HostFn`] variant.
///
/// Driven by an exhaustive `match` over [`HostFn::ALL`]: adding a variant to
/// the ABI won't compile until it has an arm here (that's the "can't forget to
/// register" guarantee). Each arm charges gas once via [`charged`] — the sole
/// entry point for `charge` — and marshals its wasm scalars through
/// [`AbiArg`]/[`AbiRet`] before calling straight into the [`HostFunctions`]
/// trait object held in the [`Store`].
pub(crate) fn register_host_functions(linker: &mut Linker<VmState<'_>>) -> Result<(), String> {
    fn link_err(e: wasmi::errors::LinkerError) -> String {
        format!("register import: {e}")
    }

    // TODO: think on how to make it better
    for &op in HostFn::ALL {
        match op {
            HostFn::GetLedgerSqn => linker.func_wrap(
                HOST_MODULE,
                op.spec().name,
                |mut caller: Caller<'_, VmState<'_>>, out_ptr: i32, out_len: i32| -> i32 {
                    to_wasm_i32(charged(&mut caller, HostFn::GetLedgerSqn, |c| {
                        let __ret = c.data().host.get_ledger_sqn()?;
                        <[u8; 4] as AbiRet>::write(__ret, c, (out_ptr, out_len))
                    }))
                },
            ),
            HostFn::GetCurrentLedgerObjField => linker.func_wrap(
                HOST_MODULE,
                op.spec().name,
                |mut caller: Caller<'_, VmState<'_>>,
                 field: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    to_wasm_i32(charged(
                        &mut caller,
                        HostFn::GetCurrentLedgerObjField,
                        |c| {
                            let __ret = c.data().host.get_current_ledger_obj_field(field)?;
                            <Vec<u8> as AbiRet>::write(__ret, c, (out_ptr, out_len))
                        },
                    ))
                },
            ),
            HostFn::Sha512Half => linker.func_wrap(
                HOST_MODULE,
                op.spec().name,
                |mut caller: Caller<'_, VmState<'_>>,
                 data_ptr: i32,
                 data_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    to_wasm_i32(charged(&mut caller, HostFn::Sha512Half, |c| {
                        let data = <Vec<u8> as AbiArg>::read(c, (data_ptr, data_len))?;
                        let __ret = c.data().host.sha512_half(&data)?;
                        <[u8; HASH_LEN] as AbiRet>::write(__ret, c, (out_ptr, out_len))
                    }))
                },
            ),
            HostFn::Trace => linker.func_wrap(
                HOST_MODULE,
                op.spec().name,
                |mut caller: Caller<'_, VmState<'_>>,
                 msg_ptr: i32,
                 msg_len: i32,
                 data_ptr: i32,
                 data_len: i32,
                 as_hex: i32|
                 -> i32 {
                    to_wasm_i32(charged(&mut caller, HostFn::Trace, |c| {
                        let msg = <String as AbiArg>::read(c, (msg_ptr, msg_len))?;
                        let data = <Vec<u8> as AbiArg>::read(c, (data_ptr, data_len))?;
                        c.data().host.trace(&msg, &data, as_hex != 0)?;
                        <() as AbiRet>::write((), c, ())
                    }))
                },
            ),
            HostFn::TraceNum => linker.func_wrap(
                HOST_MODULE,
                op.spec().name,
                |mut caller: Caller<'_, VmState<'_>>,
                 msg_ptr: i32,
                 msg_len: i32,
                 number: i64|
                 -> i32 {
                    to_wasm_i32(charged(&mut caller, HostFn::TraceNum, |c| {
                        let msg = <String as AbiArg>::read(c, (msg_ptr, msg_len))?;
                        let __ret = c.data().host.trace_num(&msg, number)?;
                        <() as AbiRet>::write(__ret, c, ())
                    }))
                },
            ),
        }
        .map_err(link_err)?;
    }
    Ok(())
}

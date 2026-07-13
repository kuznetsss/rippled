//! Guest side of the host/guest ABI: the SAME `HostFunctions` trait, but each
//! method is a wasm import call. A contract links this and either calls the
//! wrappers directly or is written generically against `HostFunctions`.
#![no_std]
extern crate alloc;

pub use host_functions::{HASH_LEN, HostError, HostFunctions, HostResult};

mod guest {
    use super::*;
    use alloc::vec::Vec;

    #[link(wasm_import_module = "host")]
    unsafe extern "C" {
        #[link_name = "ldgr_index"]
        fn ldgr_index() -> i64;
        #[link_name = "home_le_field"]
        fn home_le_field(field: i32, out_ptr: i32, out_len: i32) -> i32;
        #[link_name = "sha512_half"]
        fn sha512_half(dp: i32, dl: i32, op: i32, ol: i32) -> i32;
        #[link_name = "trace"]
        fn trace(mp: i32, ml: i32, dp: i32, dl: i32, hex: i32) -> i32;
        #[link_name = "trace_num"]
        fn trace_num(mp: i32, ml: i32, n: i64) -> i32;
    }

    /// The production guest host: every method forwards to a wasm import.
    pub struct GuestHost;

    impl HostFunctions for GuestHost {
        fn get_ledger_sqn(&self) -> HostResult<u32> {
            let r = unsafe { ldgr_index() };
            if r < 0 {
                Err(HostError::from_code(r as i32))
            } else {
                Ok(r as u32)
            }
        }
        fn get_current_ledger_obj_field(&self, field: i32) -> HostResult<Vec<u8>> {
            let mut buf = alloc::vec![0u8; 512];
            let r =
                unsafe { home_le_field(field, buf.as_mut_ptr() as usize as i32, buf.len() as i32) };
            if r < 0 {
                Err(HostError::from_code(r))
            } else {
                buf.truncate(r as usize);
                Ok(buf)
            }
        }
        fn sha512_half(&self, data: &[u8]) -> HostResult<[u8; HASH_LEN]> {
            let mut out = [0u8; HASH_LEN];
            let r = unsafe {
                sha512_half(
                    data.as_ptr() as usize as i32,
                    data.len() as i32,
                    out.as_mut_ptr() as usize as i32,
                    HASH_LEN as i32,
                )
            };
            if r < 0 {
                Err(HostError::from_code(r))
            } else {
                Ok(out)
            }
        }
        fn trace(&self, msg: &str, data: &[u8], as_hex: bool) -> HostResult<()> {
            let r = unsafe {
                trace(
                    msg.as_ptr() as usize as i32,
                    msg.len() as i32,
                    data.as_ptr() as usize as i32,
                    data.len() as i32,
                    as_hex as i32,
                )
            };
            if r < 0 {
                Err(HostError::from_code(r))
            } else {
                Ok(())
            }
        }
        fn trace_num(&self, msg: &str, number: i64) -> HostResult<()> {
            let r = unsafe { trace_num(msg.as_ptr() as usize as i32, msg.len() as i32, number) };
            if r < 0 {
                Err(HostError::from_code(r))
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use guest::GuestHost;

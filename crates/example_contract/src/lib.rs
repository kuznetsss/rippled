//! An example programmable-escrow contract, written against the shared ABI and
//! compiled to wasm32. It reads the ledger sequence, traces it, and allows the
//! escrow to finish only once the ledger is past a threshold.
#![no_std]
extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

use stdlib::{GuestHost, HostFunctions};

// Minimal never-freeing bump allocator over a fixed arena — enough to link the
// `alloc`-using parts of the ABI even though this contract does no heap work.
const ARENA: usize = 64 * 1024;
struct Bump {
    arena: UnsafeCell<[u8; ARENA]>,
    next: UnsafeCell<usize>,
}
unsafe impl Sync for Bump {}
unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let next = unsafe { &mut *self.next.get() };
        let start = (*next + layout.align() - 1) & !(layout.align() - 1);
        let end = start + layout.size();
        if end > ARENA {
            return core::ptr::null_mut();
        }
        *next = end;
        unsafe { (self.arena.get() as *mut u8).add(start) }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
#[global_allocator]
static ALLOC: Bump = Bump {
    arena: UnsafeCell::new([0; ARENA]),
    next: UnsafeCell::new(0),
};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn finish() -> i32 {
    let host = GuestHost;
    // The host fills our buffer with the 4-byte little-endian sequence number.
    let mut sqn_bytes = [0u8; 4];
    let sqn = match host.get_ledger_sqn(&mut sqn_bytes) {
        Ok(_) => u32::from_le_bytes(sqn_bytes),
        Err(e) => return e.code(),
    };
    let _ = host.trace_num("ledger_sqn", sqn as i64);
    if sqn >= 10 { 1 } else { 0 }
}

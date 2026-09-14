//! The escrow wasm VM: compile a contract, meter it, and serve its host calls.
//!
//! Every guest access goes through `abi.rs` and reaches linear memory only by
//! wasmi's bounds-checked slice operations; `forbid(unsafe_code)` makes that a
//! property rather than a claim. The cast lints are on for the same reason — on a
//! consensus path a truncating or sign-losing cast changes what a contract is
//! charged or told, so each one is argued for at its site.
#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unreachable_pub)]
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod abi;
mod args;
mod preflight;
mod register;
mod vm;

// ---------------------------------------------------------------------------
// Throwaway: the benchmark in `bench.rs` and the three lines that give it what it
// needs — the integration tests' host and module helpers, reached by the name
// they know this crate as, since they were written to link against it.
// ---------------------------------------------------------------------------
#[cfg(test)]
extern crate self as xrpl_wasm_vm;
#[cfg(test)]
mod bench;
// The lints this crate denies reach further here than they did over `tests`, which
// inherits no inner attribute of this file: the host's filler bytes are a
// `usize as u8` this code has no reason to argue for.
#[cfg(test)]
#[allow(unreachable_pub, clippy::cast_possible_truncation)]
#[path = "../tests/support/mod.rs"]
mod support;

pub use preflight::{CheckError, check};
pub use vm::{
    MAX_FIELD_BYTES, MAX_MEMORY_BYTES, MAX_MEMORY_PAGES, MAX_TABLE_ELEMENTS, RunError, RunFailure,
    RunOutcome, TRANSFER_LIMIT_BYTES, run,
};

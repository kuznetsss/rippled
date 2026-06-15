//! Rust-side of the cxx HTTP-client bridge.
//!
//! Exposes [`init_tokio_runtime`](ffi::init_tokio_runtime),
//! [`shutdown_tokio_runtime`](ffi::shutdown_tokio_runtime), and
//! [`http_request`](ffi::http_request) to C++ via the cxx bridge in `ffi`.
//! The public API surface (re-exported below) is the error types only;
//! everything else is accessed through the generated FFI header.
//!
//! Module layout:
//! - `ffi` — cxx bridge definition and lifecycle functions
//! - `completion` — `CompletionGuard` drop guard
//! - `request` — `http_request` implementation
//! - `result` — inherent constructors for bridge result types
//! - `runtime` — global Tokio runtime wrapper
//! - `error` — `Error` and `Result` types
mod client;
mod completion;
mod error;
mod ffi;
mod request;
mod result;
mod runtime;

pub use error::{Error, Result};

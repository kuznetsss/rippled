//! `cxx`-bridged HTTP client for `rippled`, backed by `reqwest` + `rustls`.
//!
//! C++ code calls into this crate through the `extern "Rust"` functions
//! declared in the `ffi` module (namespace `rs::http_client`).  A process-wide Tokio
//! multi-thread runtime and a shared `reqwest::Client` with a
//! configurable TLS context are initialized once at startup and torn down at
//! shutdown.  Individual requests are dispatched as async tasks and deliver
//! their result through a C++-owned `HttpCompletion` callback that fires
//! exactly once even if the task is dropped.

mod client;
mod completion;
mod error;
mod ffi;
mod request;
mod result;
mod runtime;

pub use error::{Error, Result};

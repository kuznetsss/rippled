use crate::runtime::Runtime;
use std::time::Duration;

#[cxx::bridge(namespace = "http_client")]
mod bridge {
    /// Flat, C-compatible mirror of [`crate::Error`]'s discriminants.
    ///
    /// Kept in sync with `Error` by the exhaustive `From<&Error>` impl in the
    /// `error` module: adding an `Error` variant without a matching
    /// `ErrorCode` variant fails to compile.
    enum ErrorCode {
        /// The operation succeeded; `Status::message` is empty.
        Ok,
        RuntimeBuild,
        AlreadyInitialized,
        NotInitialized,
        ShutDown,
        LockPoisoned,
    }

    /// Outcome of a fallible, value-less operation.
    ///
    /// `code == ErrorCode::Ok` denotes success. On failure, `message` carries
    /// the `Display` text of the originating [`crate::Error`] for diagnostics.
    /// C++ callers should lift this into `xrpl::Expected` rather than
    /// inspecting it directly.
    struct Status {
        code: ErrorCode,
        message: &'static str,
    }

    extern "Rust" {
        /// Initialize the global Tokio runtime with `threads_num` worker
        /// threads. Yields `ErrorCode::AlreadyInitialized` if the runtime is
        /// already initialized.
        fn init_tokio_runtime(threads_num: usize) -> Status;

        /// Shut down the global Tokio runtime, waiting up to `timeout_ms`
        /// milliseconds for in-flight tasks to finish. Yields
        /// `ErrorCode::NotInitialized` if the runtime was never initialized.
        fn shutdown_tokio_runtime(timeout_ms: u64) -> Status;
    }
}

pub(crate) use bridge::{ErrorCode, Status};

fn init_tokio_runtime(threads_num: usize) -> Status {
    Runtime::init(threads_num).into()
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> Status {
    Runtime::shutdown(Duration::from_millis(timeout_ms)).into()
}

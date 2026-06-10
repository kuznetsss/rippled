use crate::runtime::Runtime;
use std::time::Duration;

#[cxx::bridge(namespace = "http_client")]
mod bridge {
    extern "Rust" {
        /// Initialize the global Tokio runtime with `threads_num` worker
        /// threads. Throws if the runtime is already initialized.
        fn init_tokio_runtime(threads_num: usize) -> Result<()>;

        /// Shut down the global Tokio runtime, waiting up to `timeout_ms`
        /// milliseconds for in-flight tasks to finish. Throws if the runtime
        /// was never initialized.
        fn shutdown_tokio_runtime(timeout_ms: u64) -> Result<()>;
    }
}

fn init_tokio_runtime(threads_num: usize) -> crate::Result<()> {
    Runtime::init(threads_num)
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> crate::Result<()> {
    Runtime::shutdown(Duration::from_millis(timeout_ms))
}

use crate::client::{init_tls_context, reset_tls_context};
use crate::runtime::Runtime;
use std::time::Duration;

#[cxx::bridge(namespace = "rs::http_client")]
mod bridge {
    /// Outcome code returned by lifecycle calls (runtime / TLS init/shutdown).
    #[derive(Debug)]
    enum ErrorCode {
        Ok,
        RuntimeBuild,
        AlreadyInitialized,
        NotInitialized,
        ShutDown,
        LockPoisoned,
        CertificateReading,
        TlsConfig,
    }

    struct Status {
        code: ErrorCode,
        message: String,
    }

    #[cxx_name = "HTTPMethod"]
    enum HttpMethod {
        Get,
        Post,
    }

    #[cxx_name = "HTTPHeader"]
    struct HttpHeader {
        name: String,
        value: String,
    }

    struct Request {
        method: HttpMethod,
        url: String,
        headers: Vec<HttpHeader>,
        timeout_ms: u64,
        max_response_bytes: usize,
    }

    struct Response {
        status: u16,
        headers: Vec<HttpHeader>,
        body: Vec<u8>,
    }

    /// Outcome code for an individual HTTP request.
    #[derive(Debug)]
    enum RequestError {
        Ok,
        Timeout,
        // reqwest does not expose a typed way to tell connect/DNS/TLS apart;
        // specific cause is preserved in RequestResult::message.
        Failed,
        TooLarge,
        Canceled,
        InvalidHeader,
        NotInitialized,
    }

    /// TLS configuration passed to [`init_tls_context`].
    struct TlsConfig {
        /// When `false`, certificate and hostname verification are disabled.
        verify: bool,
        /// Path to a PEM bundle that replaces the default CA roots.
        verify_file: String,
        /// Path to a directory of PEM certificates to add on top of active roots.
        verify_dir: String,
    }

    /// Value-based result for an HTTP request; delivered via `HttpCompletion::complete`.
    struct RequestResult {
        code: RequestError,
        /// Full reqwest cause chain on failure; empty on success.
        message: String,
        /// Populated only when `code == Ok`; zero-valued on error.
        response: Response,
    }

    extern "Rust" {
        /// Initialize the shared Tokio multi-thread runtime with `threads_num` worker threads.
        ///
        /// Must be called once before any other function in this crate; returns
        /// `AlreadyInitialized` if called again.
        fn init_tokio_runtime(threads_num: usize) -> Status;

        /// Drain in-flight tasks and shut down the runtime, waiting at most `timeout_ms` ms.
        ///
        /// Safe to call when not initialized (returns `NotInitialized`).
        /// A second call after a successful shutdown is a no-op.
        fn shutdown_tokio_runtime(timeout_ms: u64) -> Status;

        /// Build and store the shared `reqwest::Client` from `config`.
        ///
        /// Replaces any previously stored client.  Must be called before
        /// [`http_request`]; returns `NotInitialized` on requests if skipped.
        fn init_tls_context(config: TlsConfig) -> Status;

        /// Drop the stored `reqwest::Client`, reverting to the uninitialized state.
        fn reset_tls_context() -> Status;

        /// Enqueue `request` on the shared Tokio runtime; `completion` is called exactly once.
        ///
        /// Ownership of `completion` transfers into the async task.  If the
        /// task is dropped before finishing (e.g. on shutdown), the
        /// `CompletionGuard` fires `completion` with `Canceled`.
        fn http_request(request: Request, body: &[u8], completion: UniquePtr<HttpCompletion>);
    }

    unsafe extern "C++" {
        include!("xrpl/net/detail/HTTPCompletion.h");

        #[namespace = "xrpl::detail"]
        #[cxx_name = "HTTPCompletion"]
        type HttpCompletion;

        fn complete(self: Pin<&mut HttpCompletion>, result: RequestResult);
    }
}

pub(crate) use bridge::{
    ErrorCode, HttpCompletion, HttpHeader, HttpMethod, Request, RequestError, RequestResult,
    Response, Status, TlsConfig,
};

// SAFETY: `HttpCompletion` is accessed only via `complete()`, which posts work
// onto a thread-safe Asio executor.  It is never aliased and is consumed
// exactly once, so moving it across thread boundaries is safe.
unsafe impl Send for HttpCompletion {}

use crate::request::http_request;

fn init_tokio_runtime(threads_num: usize) -> Status {
    Runtime::init(threads_num).into()
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> Status {
    Runtime::shutdown(Duration::from_millis(timeout_ms)).into()
}

use crate::client::{init_tls_context, reset_tls_context};
use crate::runtime::Runtime;
use std::time::Duration;

#[cxx::bridge(namespace = "rs::http_client")]
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
        CertificateReading,
        TlsConfig,
    }

    /// Outcome of a fallible, value-less operation.
    ///
    /// `code == ErrorCode::Ok` denotes success. On failure, `message` carries
    /// the `Display` text of the originating [`crate::Error`] for diagnostics.
    /// C++ callers should lift this into `xrpl::Expected` rather than
    /// inspecting it directly.
    struct Status {
        code: ErrorCode,
        message: String,
    }

    /// HTTP method for a request.
    #[cxx_name = "HTTPMethod"]
    enum HttpMethod {
        Get,
        Post,
    }

    /// A single HTTP header name/value pair.
    #[cxx_name = "HTTPHeader"]
    struct HttpHeader {
        name: String,
        value: String,
    }

    /// Parameters for an outgoing HTTP request.
    struct Request {
        method: HttpMethod,
        url: String,
        headers: Vec<HttpHeader>,
        body: Vec<u8>,
        timeout_ms: u64,
        max_response_bytes: usize,
    }

    /// An HTTP response returned from Rust to C++.
    struct Response {
        status: u16,
        headers: Vec<HttpHeader>,
        body: Vec<u8>,
    }

    struct TlsConfig {
        verify: bool,
        verify_file: String,
        verify_dir: String,
    }

    /// Per-request error kinds delivered to `HttpCompletion::complete`.
    ///
    /// Distinct from the lifecycle [`ErrorCode`] (init/shutdown/enqueue):
    /// these codes describe what went wrong *during* an in-flight HTTP request.
    /// Mapped to `boost::system::errc` values on the C++ side.
    enum RequestError {
        /// Request completed successfully.
        Ok,
        /// The request timed out.
        Timeout,
        /// TCP connection could not be established.
        Connect,
        /// DNS resolution failed.
        Dns,
        /// TLS handshake or certificate error.
        Tls,
        /// Server returned an unexpected HTTP status.
        BadStatus,
        /// Response body exceeded `max_response_bytes`.
        TooLarge,
        /// Request was cancelled (e.g. runtime shutdown dropped the task).
        Canceled,
    }

    /// The outcome of an HTTP request, delivered to `HttpCompletion::complete`.
    ///
    /// On success `code == RequestError::Ok` and `response` is populated.
    /// On failure `code` indicates the error kind and `message` carries a
    /// human-readable description.
    struct RequestResult {
        code: RequestError,
        message: String,
        response: Response,
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

        /// Enqueue an async HTTP request on the Tokio runtime.
        ///
        /// Ownership of `completion` moves into Rust.  When the request
        /// finishes, Rust calls `completion->complete(result)` and then drops
        /// the `UniquePtr`, which invokes the virtual destructor and frees the
        /// concrete `HTTPCompletionImpl<Handler>`.  On enqueue failure the
        /// `UniquePtr` is dropped before the task runs and the `CompletionGuard`
        /// calls `complete` with `RequestError::Canceled` — C++ needs no
        /// separate failure handling.
        fn http_request(req: Request, completion: UniquePtr<HttpCompletion>);

        fn init_tls_context(config: &TlsConfig) -> Status;
        fn reset_tls_context() -> Status;
    }

    unsafe extern "C++" {
        include!("xrpl/net/detail/HTTPCompletion.h");

        #[namespace = "xrpl::detail"]
        #[cxx_name = "HTTPCompletion"]
        type HttpCompletion;

        /// Post the stored Asio handler onto its associated executor with the
        /// given result.  Called by Rust (from a Tokio worker thread) when an
        /// HTTP request completes or is canceled.
        fn complete(self: Pin<&mut HttpCompletion>, result: RequestResult);
    }
}

pub(crate) use bridge::{
    ErrorCode, HttpCompletion, HttpHeader, Request, RequestError, RequestResult, Response, Status,
    TlsConfig,
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

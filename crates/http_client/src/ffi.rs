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
    #[derive(Debug)]
    enum ErrorCode {
        /// The operation succeeded; `Status::message` is empty.
        Ok,
        RuntimeBuild,
        AlreadyInitialized,
        NotInitialized,
        ShutDown,
        LockPoisoned,
        /// A TLS certificate file or directory could not be read from disk.
        CertificateReading,
        /// The TLS client context could not be built (bad config or cert parse
        /// error).
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
        timeout_ms: u64,
        max_response_bytes: usize,
    }

    /// An HTTP response returned from Rust to C++.
    struct Response {
        status: u16,
        headers: Vec<HttpHeader>,
        body: Vec<u8>,
    }

    /// Per-request error kinds delivered to `HttpCompletion::complete`.
    ///
    /// Distinct from the lifecycle [`ErrorCode`] (init/shutdown/enqueue):
    /// these codes describe what went wrong *during* an in-flight HTTP request.
    /// Mapped to `boost::system::errc` values on the C++ side.
    #[derive(Debug)]
    enum RequestError {
        /// Request completed successfully.
        Ok,
        /// The request timed out.
        Timeout,
        /// Any other failure while performing the request — connection, DNS,
        /// TLS, malformed request, mid-stream body error, etc.  reqwest does
        /// not expose a typed way to tell these apart, so they share one code;
        /// the specific cause is preserved in `RequestResult::message`.
        Failed,
        /// Response body exceeded `max_response_bytes`.
        TooLarge,
        /// Request was cancelled (e.g. runtime shutdown dropped the task).
        Canceled,
        /// A request was issued before [`init_tls_context`] was called.
        ///
        /// The global `reqwest::Client` has not been initialised; the caller
        /// must call `init_tls_context` before issuing requests.
        NotInitialized,
    }

    /// TLS / certificate-verification parameters for [`init_tls_context`].
    ///
    /// Mirrors the fields of `HTTPClientSSLContext` on the C++ side so that
    /// the same configuration can be forwarded across the FFI boundary without
    /// an additional mapping layer.
    struct TlsConfig {
        /// When `false`, TLS certificate verification is completely disabled
        /// (`danger_accept_invalid_certs`).  When `true`, certificates are
        /// verified according to `verify_file` / `verify_dir` / system roots.
        verify: bool,
        /// Path to a PEM bundle that **replaces** the default CA roots.
        /// Empty string means "use the platform / webpki roots instead".
        verify_file: String,
        /// Path to a directory of PEM certificates to add on top of whatever
        /// roots are in effect.  Each file in the directory is attempted; files
        /// that are not valid PEM are skipped silently.
        verify_dir: String,
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

        /// Build and store the global `reqwest::Client` from `config`.
        ///
        /// This must be called **before** any [`http_request`] call.  Calling
        /// it again while a client is already stored atomically replaces it
        /// (safe to call at reconfiguration time).
        ///
        /// Returns `ErrorCode::CertRead` if a certificate file/directory could
        /// not be read, `ErrorCode::TlsConfig` if the client could not be
        /// built.
        fn init_tls_context(config: TlsConfig) -> Status;

        /// Drop the stored `reqwest::Client`.
        ///
        /// After this call, [`http_request`] will return
        /// `RequestError::NotInitialized` until [`init_tls_context`] is called
        /// again.  A no-op (returns `Ok`) if no client is currently stored.
        fn reset_tls_context() -> Status;

        /// Enqueue an async HTTP request on the Tokio runtime.
        ///
        /// Ownership of `completion` moves into Rust.  When the request
        /// finishes, Rust calls `completion->complete(result)` and then drops
        /// the `UniquePtr`, which invokes the virtual destructor and frees the
        /// concrete `HTTPCompletionImpl<Handler>`.  On enqueue failure the
        /// `UniquePtr` is dropped before the task runs and the `CompletionGuard`
        /// calls `complete` with `RequestError::Canceled` — C++ needs no
        /// separate failure handling.
        fn http_request(request: Request, body: &[u8], completion: UniquePtr<HttpCompletion>);
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

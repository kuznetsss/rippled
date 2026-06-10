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

    /// HTTP method for a request.
    enum HttpMethod {
        Get,
        Post,
    }

    /// A single HTTP header name/value pair.
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

    /// The outcome of an HTTP request, delivered to `resume_http_request`.
    ///
    /// On success `code == ErrorCode::Ok` and `response` is populated.
    /// On failure `code` indicates the error kind and `message` carries a
    /// human-readable description.
    struct RequestResult {
        code: ErrorCode,
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
        /// When the request completes, `resume_http_request(completion, result)`
        /// is called from a Tokio worker thread. The `completion` token is an
        /// opaque `usize` that the C++ side uses to resume its coroutine/callback.
        ///
        /// Returns `ErrorCode::Ok` immediately if the task was successfully
        /// enqueued, or an error `Status` if the runtime is unavailable.
        fn http_request(req: Request, completion: usize) -> Status;
    }

    unsafe extern "C++" {
        include!("xrpl/net/HttpClientCallback.h");

        /// Called by Rust (from a Tokio worker thread) when an HTTP request
        /// completes.  The `completion` token identifies the C++ continuation;
        /// `result` carries the response or error details.
        unsafe fn resume_http_request(completion: usize, result: RequestResult);
    }
}

pub(crate) use bridge::{ErrorCode, HttpHeader, Request, RequestResult, Response, Status};

fn init_tokio_runtime(threads_num: usize) -> Status {
    Runtime::init(threads_num).into()
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> Status {
    Runtime::shutdown(Duration::from_millis(timeout_ms)).into()
}

fn http_request(req: Request, completion: usize) -> Status {
    Runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = Response {
            status: 200,
            headers: vec![HttpHeader {
                name: "x-stub".into(),
                value: "true".into(),
            }],
            body: format!("stub response for {}", req.url).into_bytes(),
        };

        let result = RequestResult {
            code: ErrorCode::Ok,
            message: String::new(),
            response,
        };

        // SAFETY: `completion` is a valid `*mut StubCompletion` (or equivalent
        // C++ object) for the lifetime of this call; the C++ side ensures the
        // object outlives the async task.
        unsafe { bridge::resume_http_request(completion, result) };
    })
    .into()
}

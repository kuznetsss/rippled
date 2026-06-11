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

    /// Per-request error kinds delivered to `resume_http_request`.
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

    /// The outcome of an HTTP request, delivered to `resume_http_request`.
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

pub(crate) use bridge::{ErrorCode, HttpHeader, Request, RequestError, RequestResult, Response, Status};

fn init_tokio_runtime(threads_num: usize) -> Status {
    Runtime::init(threads_num).into()
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> Status {
    Runtime::shutdown(Duration::from_millis(timeout_ms)).into()
}

/// A drop guard that calls `resume_http_request` with `Canceled` if the
/// async task is dropped before it completes normally.
///
/// Constructed as the *first* statement inside the spawned task body so that
/// on enqueue failure (task never starts) the guard is never created and C++
/// owns the failure path — preventing any double-completion.
///
/// Residual narrow race: a task that is enqueued but dropped before its first
/// poll will trigger the `Drop` path. This is an accepted limitation for now;
/// per-operation cancellation is deferred to a future iteration.
struct CompletionGuard {
    completion: usize,
    disarmed: bool,
}

impl CompletionGuard {
    fn new(completion: usize) -> Self {
        Self {
            completion,
            disarmed: false,
        }
    }

    /// Disarm and complete with the given result (happy path).
    ///
    /// # Safety
    /// `completion` must be a valid pointer cast to `usize` that the C++ side
    /// passed to `http_request`. C++ guarantees the pointed-to object outlives
    /// this call.
    unsafe fn complete(mut self, result: RequestResult) {
        self.disarmed = true;
        // SAFETY: forwarded from caller's safety contract.
        unsafe { bridge::resume_http_request(self.completion, result) };
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if !self.disarmed {
            // Task was dropped before completing — signal Canceled to C++.
            let result = RequestResult {
                code: RequestError::Canceled,
                message: String::from("request task was dropped"),
                response: Response {
                    status: 0,
                    headers: vec![],
                    body: vec![],
                },
            };
            // SAFETY: same contract as in `complete` — the C++ pointer is
            // still valid because the task was either mid-flight (runtime
            // still alive) or the runtime is shutting down and C++ holds the
            // State until we call back.
            unsafe { bridge::resume_http_request(self.completion, result) };
        }
    }
}

fn http_request(req: Request, completion: usize) -> Status {
    Runtime::spawn(async move {
        // The guard MUST be the first thing created inside the task so that
        // if this task is dropped (e.g. runtime shutdown_timeout) C++ is
        // notified via Canceled.  On enqueue failure the task body never
        // runs, so the guard is never constructed and C++ handles that path.
        let guard = CompletionGuard::new(completion);

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
            code: RequestError::Ok,
            message: String::new(),
            response,
        };

        // SAFETY: `completion` is a valid C++ completion-State pointer cast
        // to `usize`; the C++ side guarantees the object outlives this call.
        unsafe { guard.complete(result) };
    })
    .into()
}

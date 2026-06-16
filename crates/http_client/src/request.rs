//! HTTP request execution: FFI entry point and pure async logic.
//!
//! The module is split into two layers so the network logic can be unit-tested
//! without involving any C++ types:
//!
//! - [`execute`] is a pure `async fn` that takes only Rust types and returns
//!   `Result<Response, RequestFailure>`.  It can be called from
//!   `#[tokio::test]` directly.
//! - [`http_request`] is the thin FFI wrapper that creates a
//!   [`CompletionGuard`], spawns the task, and bridges the result back to C++.

use std::time::Duration;

use crate::client;
use crate::completion::CompletionGuard;
use crate::ffi::{
    HttpCompletion, HttpHeader, HttpMethod, Request, RequestError, RequestResult, Response,
};
use crate::runtime::Runtime;
use cxx::UniquePtr;

// ---------------------------------------------------------------------------
// Internal error type
// ---------------------------------------------------------------------------

/// A request-level failure, carrying the error discriminant and a human
/// message.  This is an internal type; it is converted to [`RequestResult`]
/// exactly once at the FFI boundary via the [`From`] impl below.
pub(crate) struct RequestFailure {
    pub(crate) code: RequestError,
    pub(crate) message: String,
}

// ---------------------------------------------------------------------------
// Single conversion point: Result<Response, RequestFailure> → RequestResult
// ---------------------------------------------------------------------------

impl From<Result<Response, RequestFailure>> for RequestResult {
    fn from(r: Result<Response, RequestFailure>) -> Self {
        match r {
            Ok(response) => RequestResult {
                code: RequestError::Ok,
                message: String::new(),
                response,
            },
            Err(f) => RequestResult {
                code: f.code,
                message: f.message,
                response: Response::empty(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// FFI entry point
// ---------------------------------------------------------------------------

/// Enqueue an async HTTP request on the Tokio runtime.
///
/// See the `#[cxx::bridge]` doc comment on `http_request` for the ownership
/// contract and the full description of the cancellation guarantee.
/// The cancellation-on-drop guarantee is implemented by [`CompletionGuard`];
/// see that type's doc comment for the load-bearing detail about captured vs.
/// body-local state.
pub(crate) fn http_request(request: Request, body: &[u8], completion: UniquePtr<HttpCompletion>) {
    // Guard must be captured state — see CompletionGuard's doc comment.
    let guard = CompletionGuard::new(completion);
    let body = body.to_vec();
    // Enqueue failure is propagated through CompletionGuard (Canceled on drop).
    let _ = Runtime::spawn(async move {
        let result = execute(request, body).await;
        guard.complete(result.into());
    });
}

// ---------------------------------------------------------------------------
// Pure async logic (unit-testable — no C++ types)
// ---------------------------------------------------------------------------

/// Execute an HTTP request and return a [`Response`] on success or a
/// [`RequestFailure`] on error.
///
/// This function is deliberately free of C++ types so it can be called from
/// `#[tokio::test]` without linking against the cxx bridge stubs.
///
/// # Error mapping
///
/// | Condition | [`RequestError`] |
/// |-----------|-----------------|
/// | No client initialised | `NotInitialized` |
/// | Timeout | `Timeout` |
/// | Any other transport failure (connect, DNS, TLS, URL parse, …) | `Failed` (cause in `message`) |
/// | Body exceeds `max_response_bytes` | `TooLarge` |
/// | Non-2xx status | `Ok` (status is surfaced in `Response`) |
pub(crate) async fn execute(request: Request, body: Vec<u8>) -> Result<Response, RequestFailure> {
    // ── 1. Obtain the shared client ─────────────────────────────────────────
    let client = client::get().map_err(|_| RequestFailure {
        code: RequestError::NotInitialized,
        message: "TLS context has not been initialised; call init_tls_context first".to_owned(),
    })?;

    // ── 2. Build the reqwest request ────────────────────────────────────────
    let method = match request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        _ => reqwest::Method::GET,
    };

    let mut request_builder = client.request(method, &*request.url);
    request_builder = request_builder.timeout(Duration::from_millis(request.timeout_ms));

    for h in &request.headers {
        let name = match reqwest::header::HeaderName::from_bytes(h.name.as_bytes()) {
            Ok(n) => n,
            // TODO: return an error
            Err(_) => continue, // skip malformed header names
        };
        let value = match reqwest::header::HeaderValue::from_bytes(h.value.as_bytes()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        request_builder = request_builder.header(name, value);
    }

    if !body.is_empty() {
        request_builder = request_builder.body(body);
    }

    // ── 3. Send ─────────────────────────────────────────────────────────────
    let resp = request_builder.send().await.map_err(map_reqwest_error)?;

    // ── 4. Harvest status + response headers ────────────────────────────────
    let status = resp.status().as_u16();
    let resp_headers: Vec<HttpHeader> = resp
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let v = value.to_str().ok()?;
            Some(HttpHeader {
                name: name.as_str().to_owned(),
                value: v.to_owned(),
            })
        })
        .collect();

    // ── 5. Stream body with cap ─────────────────────────────────────────────
    let max = request.max_response_bytes;
    // Right-size the buffer from Content-Length when present, clamped to the
    // cap so a small response does not eagerly allocate the full ceiling and a
    // lying/huge Content-Length cannot over-allocate.  Unknown length (chunked)
    // falls back to growing on demand.
    let initial = resp
        .content_length()
        .map(|n| (n as usize).min(max))
        .unwrap_or(0);
    let mut body: Vec<u8> = Vec::with_capacity(initial);
    let mut stream = resp;

    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > max {
                    return Err(RequestFailure {
                        code: RequestError::TooLarge,
                        message: "response body exceeded max_response_bytes".to_owned(),
                    });
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => return Err(map_reqwest_error(e)),
        }
    }

    Ok(Response {
        status,
        headers: resp_headers,
        body,
    })
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Convert a `reqwest::Error` into a [`RequestFailure`].
///
/// reqwest only exposes typed predicates for a handful of phases (timeout,
/// connect, builder, body, …); it has no structured way to tell a DNS failure
/// from a TLS error — those distinctions live as opaque, version-dependent text
/// deep in the hyper/rustls source chain.  Rather than scrape that text (which
/// silently breaks when the wording changes), we surface only `Timeout`, the
/// one distinction reqwest reports reliably, and fold every other failure into
/// `Failed`.  The specific cause is preserved verbatim in the `message`, taken
/// from `reqwest::Error::to_string()`.
fn map_reqwest_error(e: reqwest::Error) -> RequestFailure {
    let code = if e.is_timeout() {
        RequestError::Timeout
    } else {
        RequestError::Failed
    };
    RequestFailure {
        code,
        message: e.to_string(),
    }
}

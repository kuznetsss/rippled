//! HTTP request execution: FFI entry point and pure async logic.
//!
//! The module is split into two layers so the network logic can be unit-tested
//! without involving any C++ types:
//!
//! - [`execute`] is a pure `async fn` that takes only Rust types and returns
//!   a [`RequestResult`].  It can be called from `#[tokio::test]` directly.
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
        guard.complete(result);
    });
}

// ---------------------------------------------------------------------------
// Pure async logic (unit-testable — no C++ types)
// ---------------------------------------------------------------------------

/// Execute an HTTP request and return a [`RequestResult`].
///
/// This function is deliberately free of C++ types so it can be called from
/// `#[tokio::test]` without linking against the cxx bridge stubs.
///
/// # Error mapping
///
/// | Condition | [`RequestError`] |
/// |-----------|-----------------|
/// | No client initialised | `NotInitialized` |
/// | DNS failure | `Dns` |
/// | Connection refused / TCP error | `Connect` |
/// | Timeout | `Timeout` |
/// | TLS handshake / certificate | `Tls` |
/// | Body exceeds `max_response_bytes` | `TooLarge` |
/// | URL parse error | `Connect` (closest semantic match) |
/// | Non-2xx status | `Ok` (status is surfaced in `Response`) |
pub(crate) async fn execute(request: Request, body: Vec<u8>) -> RequestResult {
    // ── 1. Obtain the shared client ─────────────────────────────────────────
    let client = match client::get() {
        Ok(c) => c,
        Err(_) => {
            return RequestResult::error(
                RequestError::NotInitialized,
                "TLS context has not been initialised; call init_tls_context first",
            );
        }
    };

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
    let resp = match request_builder.send().await {
        Ok(r) => r,
        Err(e) => return RequestResult::error(map_send_error(&e), &e.to_string()),
    };

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
                    return RequestResult::error(
                        RequestError::TooLarge,
                        "response body exceeded max_response_bytes",
                    );
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => return RequestResult::error(map_send_error(&e), &e.to_string()),
        }
    }

    RequestResult::ok(Response {
        status,
        headers: resp_headers,
        body,
    })
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Map a `reqwest::Error` to the closest [`RequestError`] discriminant.
///
/// The mapping is best-effort: reqwest does not expose a structured error
/// hierarchy, so we walk `Error::source()` looking for recognisable types
/// and fall back to [`RequestError::Connect`] when uncertain.
fn map_send_error(e: &reqwest::Error) -> RequestError {
    if e.is_timeout() {
        return RequestError::Timeout;
    }

    // Walk the source chain for more specific causes.
    use std::error::Error as StdError;
    let mut source = e.source();
    while let Some(s) = source {
        let desc = s.to_string().to_lowercase();
        if desc.contains("dns")
            || desc.contains("resolve")
            || desc.contains("no such host")
            || desc.contains("name or service not known")
        {
            return RequestError::Dns;
        }
        if desc.contains("tls")
            || desc.contains("certificate")
            || desc.contains("handshake")
            || desc.contains("rustls")
            || desc.contains("invalid cert")
        {
            return RequestError::Tls;
        }
        source = s.source();
    }

    // reqwest's is_connect() covers connection-refused and similar TCP errors.
    if e.is_connect() {
        return RequestError::Connect;
    }

    // URL parse errors surface as neither timeout nor connect.
    if e.is_builder() {
        return RequestError::Connect;
    }

    RequestError::Connect
}

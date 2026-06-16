use std::time::Duration;

use crate::client;
use crate::completion::CompletionGuard;
use crate::ffi::{
    HttpCompletion, HttpHeader, HttpMethod, Request, RequestError, RequestResult, Response,
};
use crate::runtime::Runtime;
use cxx::UniquePtr;

pub(crate) struct RequestFailure {
    pub(crate) code: RequestError,
    pub(crate) message: String,
}

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

pub(crate) fn http_request(request: Request, body: &[u8], completion: UniquePtr<HttpCompletion>) {
    // Guard must be captured state, not a body-local — see CompletionGuard.
    let guard = CompletionGuard::new(completion);
    let body = body.to_vec();
    let _ = Runtime::spawn(async move {
        let result = execute(request, body).await;
        guard.complete(result.into());
    });
}

/// Pure async entry point — no C++ types, directly testable with `#[tokio::test]`.
pub(crate) async fn execute(request: Request, body: Vec<u8>) -> Result<Response, RequestFailure> {
    let client = client::get().map_err(|_| RequestFailure {
        code: RequestError::NotInitialized,
        message: "TLS context has not been initialised; call init_tls_context first".to_owned(),
    })?;

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
            Err(_) => continue,
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

    let resp = request_builder.send().await.map_err(map_reqwest_error)?;

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

    let max = request.max_response_bytes;
    // Clamp Content-Length hint to the cap to avoid over-allocating on a lying server.
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

/// Map a reqwest error to a RequestFailure.
///
/// Only `Timeout` is reported with a distinct code; reqwest has no reliable way
/// to distinguish connect/DNS/TLS failures, so everything else maps to `Failed`
/// with the original message preserved.
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

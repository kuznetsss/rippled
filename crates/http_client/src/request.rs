use std::time::Duration;

use crate::client;
use crate::completion::CompletionGuard;
use crate::error::RequestFailure;
use crate::ffi::{HttpCompletion, HttpHeader, HttpMethod, Request, Response};
use crate::runtime::Runtime;
use cxx::UniquePtr;

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
    let client = client::get().map_err(|_| RequestFailure::NotInitialized)?;

    let method = match request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        _ => reqwest::Method::GET,
    };

    let mut request_builder = client.request(method, &*request.url);
    request_builder = request_builder.timeout(Duration::from_millis(request.timeout_ms));

    for h in &request.headers {
        let name = reqwest::header::HeaderName::from_bytes(h.name.as_bytes())
            .map_err(|_| RequestFailure::InvalidHeaderName(h.name.clone()))?;
        let value = reqwest::header::HeaderValue::from_bytes(h.value.as_bytes())
            .map_err(|_| RequestFailure::InvalidHeaderValue(h.value.clone()))?;
        request_builder = request_builder.header(name, value);
    }

    if !body.is_empty() {
        request_builder = request_builder.body(body);
    }

    let response = request_builder.send().await?;

    let status = response.status().as_u16();
    let resp_headers: Vec<HttpHeader> = response
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

    // Clamp Content-Length hint to the cap to avoid over-allocating on a lying server.
    let initial = response
        .content_length()
        .map(|n| (n as usize).min(request.max_response_bytes))
        .unwrap_or(0);
    let mut body: Vec<u8> = Vec::with_capacity(initial);
    let mut stream = response;

    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > request.max_response_bytes {
                    return Err(RequestFailure::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(Response {
        status,
        headers: resp_headers,
        body,
    })
}

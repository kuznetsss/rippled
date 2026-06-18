//! HTTP request execution: FFI entry point and the underlying async logic.

use std::time::Duration;

use crate::client;
use crate::completion::CompletionGuard;
use crate::error::RequestFailure;
use crate::ffi::{HttpCompletion, HttpHeader, HttpMethod, Request, Response};
use crate::runtime::Runtime;
use cxx::UniquePtr;

/// FFI entry point: spawns an async task on the shared runtime and guarantees
/// `completion` fires exactly once via [`CompletionGuard`].
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
    execute_with(&client, request, body).await
}

/// Pure async worker — takes an already-resolved client, so tests can inject
/// a local `reqwest::Client` without touching the `CLIENT` global.
async fn execute_with(
    client: &reqwest::Client,
    request: Request,
    body: Vec<u8>,
) -> Result<Response, RequestFailure> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a minimal GET request pointing at `url`.
    fn req(url: String, max_response_bytes: usize) -> Request {
        Request {
            method: HttpMethod::Get,
            url,
            headers: vec![],
            timeout_ms: 5000,
            max_response_bytes,
        }
    }

    // Helper that constructs a plain reqwest::Client without touching CLIENT global.
    fn local_client() -> reqwest::Client {
        reqwest::Client::builder().build().unwrap()
    }

    /// Happy path: 200 with a body and a custom response header.
    #[tokio::test]
    async fn happy_path_200_with_body_and_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-reply", "yes")
                    .set_body_string("hello"),
            )
            .mount(&server)
            .await;

        let client = local_client();
        let resp = execute_with(&client, req(server.uri(), 1_000_000), vec![])
            .await
            .unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
        // The custom header must appear in the response headers.
        let header = resp
            .headers
            .iter()
            .find(|h| h.name == "x-reply")
            .expect("x-reply header missing");
        assert_eq!(header.value, "yes");
    }

    /// Body exceeding `max_response_bytes` must yield `TooLarge`.
    #[tokio::test]
    async fn max_response_bytes_enforcement() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(100)))
            .mount(&server)
            .await;

        let client = local_client();
        // Allow only 10 bytes; the 100-byte body must trip the limit.
        let result = execute_with(&client, req(server.uri(), 10), vec![]).await;
        assert!(matches!(result, Err(RequestFailure::TooLarge)));
    }

    /// A header with an invalid value (embedded newline) must be rejected
    /// before the network is even hit.
    #[tokio::test]
    async fn invalid_header_value_rejected() {
        // No mock server needed — the error fires during header construction.
        let client = local_client();
        let mut r = req("http://127.0.0.1:1".to_string(), 1_000_000);
        r.headers.push(HttpHeader {
            name: "x-test".to_string(),
            value: "bad\nvalue".to_string(),
        });
        let result = execute_with(&client, r, vec![]).await;
        assert!(matches!(result, Err(RequestFailure::InvalidHeaderValue(_))));
    }

    /// A header with an invalid name (embedded space) must also be rejected.
    #[tokio::test]
    async fn invalid_header_name_rejected() {
        let client = local_client();
        let mut r = req("http://127.0.0.1:1".to_string(), 1_000_000);
        r.headers.push(HttpHeader {
            name: "bad header".to_string(),
            value: "value".to_string(),
        });
        let result = execute_with(&client, r, vec![]).await;
        assert!(matches!(result, Err(RequestFailure::InvalidHeaderName(_))));
    }
}

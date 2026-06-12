//! Implementation of the `http_request` FFI entry point.

use crate::completion::CompletionGuard;
use crate::ffi::{HttpCompletion, HttpHeader, Request, RequestResult, Response};
use crate::runtime::Runtime;
use cxx::UniquePtr;

/// Enqueue an async HTTP request on the Tokio runtime.
///
/// See the `#[cxx::bridge]` doc comment on `http_request` for the ownership
/// contract and the full description of the cancellation guarantee.
/// The cancellation-on-drop guarantee is implemented by [`CompletionGuard`];
/// see that type's doc comment for the load-bearing detail about captured vs.
/// body-local state.
pub(crate) fn http_request(req: Request, completion: UniquePtr<HttpCompletion>) {
    // Guard must be captured state — see CompletionGuard's doc comment.
    let guard = CompletionGuard::new(completion);
    // Enqueue failure is propagated through CompletionGuard (Canceled on drop).
    let _ = Runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = Response {
            status: 200,
            headers: vec![HttpHeader {
                name: "x-stub".into(),
                value: "true".into(),
            }],
            body: format!("stub response for {}", req.url).into_bytes(),
        };

        guard.complete(RequestResult::ok(response));
    });
}

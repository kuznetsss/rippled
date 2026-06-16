//! Convenience constructors for cxx bridge result types.
//!
//! The bridge structs (`Response`, `RequestResult`) are ordinary Rust types
//! defined by the cxx macro. Inherent impls added here keep construction sites
//! tidy and give a single place to update if the struct fields change.
//!
//! Success and generic-error construction is handled by the
//! `From<Result<Response, RequestFailure>> for RequestResult` impl in
//! `request.rs`. Only the `Canceled` path (used by `CompletionGuard::drop`)
//! is kept here.

use crate::ffi::{RequestError, RequestResult, Response};

impl Response {
    /// Construct an empty response (used when no HTTP exchange took place).
    pub(crate) fn empty() -> Self {
        Response {
            status: 0,
            headers: vec![],
            body: vec![],
        }
    }
}

impl RequestResult {
    /// Construct a canceled result with a human-readable `message`.
    pub(crate) fn canceled(message: &str) -> Self {
        RequestResult {
            code: RequestError::Canceled,
            message: message.to_owned(),
            response: Response::empty(),
        }
    }
}

//! Convenience constructors for cxx bridge result types.
//!
//! The bridge structs (`Response`, `RequestResult`) are ordinary Rust types
//! defined by the cxx macro. Inherent impls added here keep construction sites
//! tidy and give a single place to update if the struct fields change.

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
    /// Construct a successful result carrying the given response.
    pub(crate) fn ok(response: Response) -> Self {
        RequestResult {
            code: RequestError::Ok,
            message: String::new(),
            response,
        }
    }

    /// Construct a canceled result with a human-readable `message`.
    pub(crate) fn canceled(message: &str) -> Self {
        RequestResult {
            code: RequestError::Canceled,
            message: message.to_owned(),
            response: Response::empty(),
        }
    }
}

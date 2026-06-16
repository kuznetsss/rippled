use crate::ffi::{RequestError, RequestResult, Response};

impl Response {
    pub(crate) fn empty() -> Self {
        Response {
            status: 0,
            headers: vec![],
            body: vec![],
        }
    }
}

impl RequestResult {
    pub(crate) fn canceled(message: &str) -> Self {
        RequestResult {
            code: RequestError::Canceled,
            message: message.to_owned(),
            response: Response::empty(),
        }
    }
}

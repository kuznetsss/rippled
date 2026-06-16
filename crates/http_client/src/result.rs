use crate::ffi::Response;

impl Response {
    pub(crate) fn empty() -> Self {
        Response {
            status: 0,
            headers: vec![],
            body: vec![],
        }
    }
}

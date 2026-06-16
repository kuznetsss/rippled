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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_response_has_zero_status() {
        let r = Response::empty();
        assert_eq!(r.status, 0);
    }

    #[test]
    fn empty_response_has_no_headers() {
        let r = Response::empty();
        assert!(r.headers.is_empty());
    }

    #[test]
    fn empty_response_has_no_body() {
        let r = Response::empty();
        assert!(r.body.is_empty());
    }
}

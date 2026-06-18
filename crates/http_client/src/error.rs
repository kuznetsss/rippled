//! Error types and their mappings to the cxx bridge result types.

use crate::ffi::{ErrorCode, RequestError, RequestResult, Response, Status};

/// Lifecycle errors for the runtime and TLS context; mapped to `ErrorCode` for FFI.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to build the Tokio runtime: {0}")]
    RuntimeBuild(#[source] std::io::Error),

    #[error("runtime is already initialized")]
    AlreadyInitialized,

    #[error("runtime is not initialized")]
    NotInitialized,

    #[error("runtime lock is poisoned")]
    LockPoisoned,

    #[error("failed to read TLS certificate: {0}")]
    CertificateReading(#[source] std::io::Error),

    #[error("TLS configuration error: {0}")]
    TlsConfig(#[source] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<&Error> for ErrorCode {
    fn from(error: &Error) -> Self {
        // Intentionally exhaustive: adding an Error variant without a matching
        // ErrorCode variant fails to compile, keeping the FFI mirror in sync.
        match error {
            Error::RuntimeBuild(_) => ErrorCode::RuntimeBuild,
            Error::AlreadyInitialized => ErrorCode::AlreadyInitialized,
            Error::NotInitialized => ErrorCode::NotInitialized,
            Error::LockPoisoned => ErrorCode::LockPoisoned,
            Error::CertificateReading(_) => ErrorCode::CertificateReading,
            Error::TlsConfig(_) => ErrorCode::TlsConfig,
        }
    }
}

impl From<Result<()>> for Status {
    fn from(result: Result<()>) -> Self {
        match result {
            Ok(()) => Status {
                code: ErrorCode::Ok,
                message: String::new(),
            },
            Err(error) => Status {
                code: (&error).into(),
                message: error.to_string(),
            },
        }
    }
}

/// Per-request errors; mapped to [`RequestError`] for FFI.
///
/// `Timeout` and `Transport` both carry the full reqwest cause string, which
/// surfaces in `RequestResult::message` on the C++ side.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RequestFailure {
    #[error("TLS context has not been initialized; call init_tls_context first")]
    NotInitialized,
    #[error("invalid HTTP header name: {0}")]
    InvalidHeaderName(String),
    #[error("invalid HTTP header value: {0}")]
    InvalidHeaderValue(String),
    #[error("response body exceeded max_response_bytes")]
    TooLarge,
    #[error("request task was dropped")]
    Canceled,
    // Both preserve the reqwest cause string; only the code differs.
    #[error(transparent)]
    Timeout(reqwest::Error),
    #[error(transparent)]
    Transport(reqwest::Error),
}

impl From<reqwest::Error> for RequestFailure {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            RequestFailure::Timeout(e)
        } else {
            RequestFailure::Transport(e)
        }
    }
}

impl From<&RequestFailure> for RequestError {
    fn from(f: &RequestFailure) -> Self {
        // Exhaustive: a new RequestFailure variant without a RequestError
        // code fails to compile, keeping the FFI mirror in sync.
        match f {
            RequestFailure::NotInitialized => RequestError::NotInitialized,
            RequestFailure::InvalidHeaderName(_) | RequestFailure::InvalidHeaderValue(_) => {
                RequestError::InvalidHeader
            }
            RequestFailure::TooLarge => RequestError::TooLarge,
            RequestFailure::Canceled => RequestError::Canceled,
            RequestFailure::Timeout(_) => RequestError::Timeout,
            RequestFailure::Transport(_) => RequestError::Failed,
        }
    }
}

impl From<std::result::Result<Response, RequestFailure>> for RequestResult {
    fn from(r: std::result::Result<Response, RequestFailure>) -> Self {
        match r {
            Ok(response) => RequestResult {
                code: RequestError::Ok,
                message: String::new(),
                response,
            },
            Err(f) => RequestResult {
                code: (&f).into(),
                message: f.to_string(),
                response: Response::empty(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_runtime_build() {
        let e = Error::RuntimeBuild(std::io::Error::other("oops"));
        assert!(matches!(ErrorCode::from(&e), ErrorCode::RuntimeBuild));
    }

    #[test]
    fn error_code_already_initialized() {
        let e = Error::AlreadyInitialized;
        assert!(matches!(ErrorCode::from(&e), ErrorCode::AlreadyInitialized));
    }

    #[test]
    fn error_code_not_initialized() {
        let e = Error::NotInitialized;
        assert!(matches!(ErrorCode::from(&e), ErrorCode::NotInitialized));
    }

    #[test]
    fn error_code_lock_poisoned() {
        let e = Error::LockPoisoned;
        assert!(matches!(ErrorCode::from(&e), ErrorCode::LockPoisoned));
    }

    #[test]
    fn error_code_certificate_reading() {
        let e = Error::CertificateReading(std::io::Error::other("bad cert"));
        assert!(matches!(ErrorCode::from(&e), ErrorCode::CertificateReading));
    }

    #[test]
    fn request_error_not_initialized() {
        let f = RequestFailure::NotInitialized;
        assert!(matches!(
            RequestError::from(&f),
            RequestError::NotInitialized
        ));
    }

    #[test]
    fn request_error_invalid_header_name() {
        // Both InvalidHeaderName and InvalidHeaderValue map to InvalidHeader.
        let f = RequestFailure::InvalidHeaderName("X-Bad".to_string());
        assert!(matches!(
            RequestError::from(&f),
            RequestError::InvalidHeader
        ));
    }

    #[test]
    fn request_error_invalid_header_value() {
        let f = RequestFailure::InvalidHeaderValue("bad\nvalue".to_string());
        assert!(matches!(
            RequestError::from(&f),
            RequestError::InvalidHeader
        ));
    }

    #[test]
    fn request_error_too_large() {
        let f = RequestFailure::TooLarge;
        assert!(matches!(RequestError::from(&f), RequestError::TooLarge));
    }

    #[test]
    fn request_error_canceled() {
        let f = RequestFailure::Canceled;
        assert!(matches!(RequestError::from(&f), RequestError::Canceled));
    }

    #[test]
    fn status_from_ok() {
        let s: Status = Ok(()).into();
        assert!(matches!(s.code, ErrorCode::Ok));
        assert_eq!(s.message, "");
    }

    #[test]
    fn status_from_err_not_initialized() {
        let s: Status = Err(Error::NotInitialized).into();
        assert!(matches!(s.code, ErrorCode::NotInitialized));
        assert!(!s.message.is_empty());
    }

    #[test]
    fn status_from_err_runtime_build() {
        let s: Status = Err(Error::RuntimeBuild(std::io::Error::other("boom"))).into();
        assert!(matches!(s.code, ErrorCode::RuntimeBuild));
        assert!(!s.message.is_empty());
    }

    #[test]
    fn request_result_from_ok_response() {
        let resp = Response {
            status: 200,
            headers: vec![],
            body: vec![1, 2, 3],
        };
        let r: RequestResult = Ok(resp).into();
        assert!(matches!(r.code, RequestError::Ok));
        assert_eq!(r.message, "");
        assert_eq!(r.response.body, vec![1u8, 2, 3]);
    }

    #[test]
    fn request_result_from_err_too_large() {
        let r: RequestResult = Err(RequestFailure::TooLarge).into();
        assert!(matches!(r.code, RequestError::TooLarge));
        assert!(!r.message.is_empty());
        // Error path delivers an empty response.
        assert_eq!(r.response.status, 0);
        assert!(r.response.body.is_empty());
    }
}

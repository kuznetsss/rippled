use crate::ffi::{ErrorCode, RequestError, RequestResult, Response, Status};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to build the Tokio runtime: {0}")]
    RuntimeBuild(#[source] std::io::Error),

    #[error("runtime is already initialized")]
    AlreadyInitialized,

    #[error("runtime is not initialized")]
    NotInitialized,

    #[error("runtime has been shut down")]
    ShutDown,

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
            Error::ShutDown => ErrorCode::ShutDown,
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

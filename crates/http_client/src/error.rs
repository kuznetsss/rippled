use crate::ffi::{ErrorCode, Status};

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

//! Error types for the `http_client` crate.

use crate::ffi::{ErrorCode, Status};

/// Errors that can occur while interacting with the [`Runtime`](crate::Runtime).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The underlying Tokio runtime could not be built.
    #[error("failed to build the Tokio runtime: {0}")]
    RuntimeBuild(#[source] std::io::Error),

    /// [`Runtime::init`](crate::Runtime::init) was called more than once.
    #[error("runtime is already initialized")]
    AlreadyInitialized,

    /// The runtime was used before [`Runtime::init`](crate::Runtime::init) was
    /// called.
    #[error("runtime is not initialized")]
    NotInitialized,

    /// The runtime was used after it had been shut down.
    #[error("runtime has been shut down")]
    ShutDown,

    /// A runtime lock was poisoned by a panic in another thread.
    #[error("runtime lock is poisoned")]
    LockPoisoned,

    /// Error reading TLS certificate.
    #[error("failed to read TLS certificate: {0}")]
    CertificateReading(#[source] std::io::Error),

    /// TLS client configuration was rejected by `reqwest`.
    #[error("TLS configuration error: {0}")]
    TlsConfig(#[source] reqwest::Error),
}

/// A specialized [`Result`] type for `http_client` operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<&Error> for ErrorCode {
    fn from(error: &Error) -> Self {
        // Intentionally exhaustive (no wildcard arm): a new `Error` variant
        // must be given a matching `ErrorCode` variant, or this fails to
        // compile. That is what keeps the FFI mirror from drifting.
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

/// Flattens a [`Result`] into the FFI [`Status`].
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

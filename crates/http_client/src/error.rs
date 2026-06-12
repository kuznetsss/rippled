//! Error types for the `http_client` crate.

use crate::ffi::{ErrorCode, Status};

/// Errors that can occur while interacting with the [`Runtime`](crate::Runtime).
#[derive(Debug)]
pub enum Error {
    /// The underlying Tokio runtime could not be built. The `Display` text is
    /// static; the dynamic cause is available via [`std::error::Error::source`].
    RuntimeBuild(std::io::Error),

    /// [`Runtime::init`](crate::Runtime::init) was called more than once.
    AlreadyInitialized,

    /// The runtime was used before [`Runtime::init`](crate::Runtime::init) was
    /// called.
    NotInitialized,

    /// The runtime was used after it had been shut down.
    ShutDown,

    /// A runtime lock was poisoned by a panic in another thread.
    LockPoisoned,
}

impl Error {
    /// Static label used by both [`Display`] and the FFI [`Status`] message.
    ///
    /// Returns `&'static str` so it crosses the FFI boundary without allocating.
    /// Single source of truth — the two representations cannot drift.
    pub(crate) const fn message(&self) -> &'static str {
        match self {
            Error::RuntimeBuild(_) => "failed to build the Tokio runtime",
            Error::AlreadyInitialized => "runtime is already initialized",
            Error::NotInitialized => "runtime is not initialized",
            Error::ShutDown => "runtime has been shut down",
            Error::LockPoisoned => "runtime lock is poisoned",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::RuntimeBuild(source) => Some(source),
            _ => None,
        }
    }
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
        }
    }
}

/// Flattens a [`Result`] into the FFI [`Status`].
///
/// Only a static `message` crosses the boundary. The dynamic OS error behind a
/// failed runtime build won't fit there, so it is written to stderr here.
impl From<Result<()>> for Status {
    fn from(result: Result<()>) -> Self {
        match result {
            Ok(()) => Status {
                code: ErrorCode::Ok,
                message: "",
            },
            Err(error) => {
                if let Error::RuntimeBuild(source) = &error {
                    eprintln!("http_client: {error}: {source}");
                }
                Status {
                    code: (&error).into(),
                    message: error.message(),
                }
            }
        }
    }
}

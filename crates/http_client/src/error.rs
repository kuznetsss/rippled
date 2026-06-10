//! Error types for the `http_client` crate.

/// Errors that can occur while interacting with the [`Runtime`](crate::Runtime).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The underlying Tokio runtime could not be built.
    #[error("failed to build the Tokio runtime")]
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
}

/// A specialized [`Result`] type for `http_client` operations.
pub type Result<T> = std::result::Result<T, Error>;

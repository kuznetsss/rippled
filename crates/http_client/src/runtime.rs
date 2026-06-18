//! Process-wide Tokio runtime, initialized once and shared across all requests.

use crate::error::{Error, Result};
use std::{
    sync::{OnceLock, RwLock},
    time::Duration,
};

/// Wrapper around a lazily-initialized, process-wide Tokio multi-thread runtime.
///
/// The `RwLock<Option<...>>` inner allows `shutdown` to take the runtime out
/// (dropping it after the timeout) without invalidating the `OnceLock` slot,
/// so that subsequent calls get `ShutDown` rather than crashing.
pub(crate) struct Runtime {
    inner: RwLock<Option<tokio::runtime::Runtime>>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

impl Runtime {
    /// Build and register the runtime; returns `AlreadyInitialized` if called again.
    pub(crate) fn init(threads_num: usize) -> Result<()> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .thread_name_fn(|| {
                static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                format!(
                    "tokio-runtime-{}",
                    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                )
            })
            .worker_threads(threads_num)
            .enable_all()
            .build()
            .map_err(Error::RuntimeBuild)?;
        RUNTIME
            .set(Runtime {
                inner: Some(rt).into(),
            })
            .map_err(|_| Error::AlreadyInitialized)
    }

    /// Calling `shutdown` a second time is a no-op (the inner runtime has
    /// already been taken).
    pub(crate) fn shutdown(timeout: Duration) -> Result<()> {
        let runtime = RUNTIME.get().ok_or(Error::NotInitialized)?;
        let inner = runtime
            .inner
            .write()
            .map_err(|_| Error::LockPoisoned)?
            .take();
        if let Some(inner) = inner {
            inner.shutdown_timeout(timeout);
        }
        Ok(())
    }

    /// Returns `Err` if the runtime is not initialized or has been shut down;
    /// the caller propagates failures via `CompletionGuard` (Canceled on drop).
    pub(crate) fn spawn<F, O>(f: F) -> Result<()>
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        let runtime = RUNTIME.get().ok_or(Error::NotInitialized)?;
        let guard = runtime.inner.read().map_err(|_| Error::LockPoisoned)?;
        let tokio_rt = guard.as_ref().ok_or(Error::ShutDown)?;
        tokio_rt.spawn(f);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end lifecycle: not-initialized → init → double-init error →
    /// spawn succeeds → shutdown → spawn fails.
    ///
    /// Uses `#[serial]` because `RUNTIME` is a process-wide `OnceLock`.
    /// No other test in this suite must touch `RUNTIME`.
    /// This is NOT a `#[tokio::test]` — the test manages the runtime itself.
    #[serial_test::serial]
    #[test]
    fn runtime_lifecycle() {
        use std::time::Duration;
        assert!(matches!(Runtime::spawn(async {}), Err(Error::NotInitialized)));
        assert!(Runtime::init(1).is_ok());
        assert!(matches!(Runtime::init(1), Err(Error::AlreadyInitialized)));
        assert!(Runtime::spawn(async {}).is_ok());
        assert!(Runtime::shutdown(Duration::from_millis(100)).is_ok());
        assert!(matches!(Runtime::spawn(async {}), Err(Error::ShutDown)));
    }
}

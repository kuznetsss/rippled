//! Process-wide Tokio runtime, lazily initialized and revivable after shutdown.

use crate::error::{Error, Result};
use std::{
    sync::{OnceLock, RwLock},
    time::Duration,
};

static RUNTIME: OnceLock<RwLock<Option<tokio::runtime::Runtime>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<tokio::runtime::Runtime>> {
    RUNTIME.get_or_init(|| RwLock::new(None))
}

/// Namespace for the process-wide Tokio multi-thread runtime.
///
/// The runtime is stored in a `RwLock<Option<...>>` behind a `OnceLock` slot
/// so that it can be shut down and re-created:
///
/// - **never initialized** — `init` builds a fresh runtime and stores it.
/// - **currently running** — `init` returns `Err(AlreadyInitialized)`.
/// - **shut down** (previously initialised, then `shutdown`) — `init` builds a
///   fresh runtime again (revival).
pub(crate) struct Runtime;

impl Runtime {
    /// Build and register the runtime.
    ///
    /// Returns `AlreadyInitialized` if the runtime is currently running.
    /// If the runtime was previously shut down, it is rebuilt (revival).
    pub(crate) fn init(threads_num: usize) -> Result<()> {
        let mut guard = slot().write().map_err(|_| Error::LockPoisoned)?;
        if guard.is_some() {
            return Err(Error::AlreadyInitialized);
        }
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
        *guard = Some(rt);
        Ok(())
    }

    /// Drain in-flight tasks and shut down the runtime, waiting at most `timeout`.
    ///
    /// Calling shutdown when not running is a no-op (`Ok(())`).
    /// After shutdown, `init` may be called again to revive the runtime.
    pub(crate) fn shutdown(timeout: Duration) -> Result<()> {
        let inner = slot().write().map_err(|_| Error::LockPoisoned)?.take();
        // Drop the write lock before blocking so concurrent spawns don't stall.
        if let Some(rt) = inner {
            rt.shutdown_timeout(timeout);
        }
        Ok(())
    }

    /// Spawn `f` on the runtime.
    ///
    /// Returns `Err(NotInitialized)` if the runtime has not been initialised
    /// or has been shut down.
    pub(crate) fn spawn<F, O>(f: F) -> Result<()>
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        let guard = slot().read().map_err(|_| Error::LockPoisoned)?;
        let tokio_rt = guard.as_ref().ok_or(Error::NotInitialized)?;
        tokio_rt.spawn(f);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end lifecycle including revival after shutdown:
    /// spawn → `NotInitialized`; init → ok; double-init → `AlreadyInitialized`;
    /// spawn → ok; shutdown → ok; spawn → `NotInitialized`;
    /// **init again (revival) → ok**; spawn → ok; shutdown → ok.
    ///
    /// Uses `#[serial]` because `RUNTIME` is a process-wide singleton.
    /// No other test in this suite must touch `RUNTIME`.
    /// This is NOT a `#[tokio::test]` — the test manages the runtime itself.
    #[serial_test::serial]
    #[test]
    fn runtime_lifecycle() {
        use std::time::Duration;

        // Not yet initialised.
        assert!(matches!(
            Runtime::spawn(async {}),
            Err(Error::NotInitialized)
        ));

        // First init succeeds.
        assert!(Runtime::init(1).is_ok());

        // Double-init is rejected while the runtime is live.
        assert!(matches!(Runtime::init(1), Err(Error::AlreadyInitialized)));

        // Spawn succeeds on a live runtime.
        assert!(Runtime::spawn(async {}).is_ok());

        // Shutdown succeeds.
        assert!(Runtime::shutdown(Duration::from_millis(100)).is_ok());

        // After shutdown, spawn fails with NotInitialized (not ShutDown).
        assert!(matches!(
            Runtime::spawn(async {}),
            Err(Error::NotInitialized)
        ));

        // Revival: init succeeds again after shutdown.
        assert!(Runtime::init(1).is_ok());

        // Spawn works on the revived runtime.
        assert!(Runtime::spawn(async {}).is_ok());

        // Clean up.
        assert!(Runtime::shutdown(Duration::from_millis(100)).is_ok());
    }
}

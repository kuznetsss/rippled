use crate::error::{Error, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};

pub(crate) struct Runtime {
    // Cached handle: spawning through it is lock-free, so the per-request hot
    // path touches no shared lock.
    handle: tokio::runtime::Handle,
    // Keeps the runtime alive; consumed by `shutdown`. Behind a Mutex only so
    // shutdown can `take()` it — never touched on the request path.
    inner: Mutex<Option<tokio::runtime::Runtime>>,
    // Cleared once shutdown starts so `spawn` reports `ShutDown` instead of
    // spawning onto a dying runtime. A read-only load on the hot path: the
    // cache line stays Shared across cores, unlike a RwLock's reader count
    // which every reader writes (cache-line ping-pong under concurrency).
    running: AtomicBool,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

impl Runtime {
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
        let handle = rt.handle().clone();
        RUNTIME
            .set(Runtime {
                handle,
                inner: Mutex::new(Some(rt)),
                running: AtomicBool::new(true),
            })
            .map_err(|_| Error::AlreadyInitialized)
    }

    /// Calling `shutdown` a second time is a no-op (the inner runtime has
    /// already been taken).
    pub(crate) fn shutdown(timeout: Duration) -> Result<()> {
        let runtime = RUNTIME.get().ok_or(Error::NotInitialized)?;
        // Stop accepting new work before tearing the runtime down.
        runtime.running.store(false, Ordering::Release);
        let inner = runtime
            .inner
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .take();
        if let Some(inner) = inner {
            inner.shutdown_timeout(timeout);
        }
        Ok(())
    }

    /// Returns `Err` if the runtime is not initialized or has been shut down;
    /// the caller propagates failures via `CompletionGuard` (Canceled on drop).
    ///
    /// The `running`/`spawn` pair is not atomic against a concurrent
    /// `shutdown`, but shutdown is process teardown — it does not race
    /// steady-state request load — so the cheaper lock-free path is sound here.
    pub(crate) fn spawn<F, O>(f: F) -> Result<()>
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        let runtime = RUNTIME.get().ok_or(Error::NotInitialized)?;
        if !runtime.running.load(Ordering::Acquire) {
            return Err(Error::ShutDown);
        }
        runtime.handle.spawn(f);
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

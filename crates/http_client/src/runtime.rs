use crate::error::{Error, Result};
use std::{
    sync::{OnceLock, RwLock},
    time::Duration,
};

/// Global, process-wide Tokio runtime.
///
/// `init` must be called exactly once before `spawn` or `shutdown`.
/// All methods are safe to call from any thread.
pub(crate) struct Runtime {
    inner: RwLock<Option<tokio::runtime::Runtime>>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

impl Runtime {
    /// Initialize the global runtime with `threads_num` worker threads.
    ///
    /// Returns `Err(Error::AlreadyInitialized)` if called more than once.
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

    /// Shut down the runtime, waiting up to `timeout` for tasks to finish.
    ///
    /// Returns `Err(Error::NotInitialized)` if `init` was never called.
    /// Calling `shutdown` a second time returns `Ok(())` (the inner runtime
    /// has already been taken).
    pub(crate) fn shutdown(timeout: Duration) -> Result<()> {
        let runtime = RUNTIME.get().ok_or(Error::NotInitialized)?;
        let inner = runtime.inner.write().map_err(|_| Error::LockPoisoned)?.take();
        if let Some(inner) = inner {
            inner.shutdown_timeout(timeout);
        }
        Ok(())
    }

    /// Enqueue a future on the runtime without waiting for it to complete.
    ///
    /// Returns `Err` if the runtime is not initialized or has been shut down;
    /// the caller is responsible for propagating any such failure (typically
    /// via a `CompletionGuard` that fires `Canceled` on drop).
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

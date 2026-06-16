use crate::error::{Error, Result};
use std::{
    sync::{OnceLock, RwLock},
    time::Duration,
};

pub(crate) struct Runtime {
    inner: RwLock<Option<tokio::runtime::Runtime>>,
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

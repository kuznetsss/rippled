//! Drop guard that guarantees `HttpCompletion::complete` fires exactly once.

use crate::ffi::{HttpCompletion, RequestResult};
use cxx::UniquePtr;

/// A drop guard that calls `HttpCompletion::complete` with `Canceled` if the
/// async task is dropped before it completes normally.
///
/// It is constructed *before* the task is spawned and moved into the task, so
/// it lives in the future's captured state rather than as a body local.  That
/// distinction is load-bearing: a future dropped before its first poll never
/// runs its body, so a guard declared as a body local would never be
/// constructed and the completion would be freed without ever firing.  As a
/// captured variable it is instead dropped on every early-out path — enqueue
/// failure (`Runtime::spawn` returns `Err` before the first poll) and the
/// runtime-shutdown race alike — and its `Drop` fires `Canceled`.  On the happy
/// path the task body consumes it via `complete`.
///
/// Per-operation cancellation is not otherwise supported; that is deferred to a
/// future iteration.
pub(crate) struct CompletionGuard {
    completion: Option<UniquePtr<HttpCompletion>>,
}

impl CompletionGuard {
    pub(crate) fn new(completion: UniquePtr<HttpCompletion>) -> Self {
        Self {
            completion: Some(completion),
        }
    }

    /// Disarm and complete with the given result (happy path).
    pub(crate) fn complete(mut self, result: RequestResult) {
        if let Some(mut c) = self.completion.take() {
            c.pin_mut().complete(result);
        }
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.completion.take() {
            // Task was dropped before completing — signal Canceled to C++.
            let result = RequestResult::canceled("request task was dropped");
            c.pin_mut().complete(result);
        }
    }
}

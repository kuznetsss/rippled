use crate::ffi::{HttpCompletion, RequestResult};
use cxx::UniquePtr;

/// Drop guard that calls `HttpCompletion::complete` with `Canceled` if the
/// async task is dropped before completing normally.
///
/// Must be constructed *before* the task is spawned and captured by the future,
/// not declared as a body-local: a future dropped before its first poll never
/// executes its body, so a body-local guard would never be constructed and the
/// completion would be freed without firing.
pub(crate) struct CompletionGuard {
    completion: Option<UniquePtr<HttpCompletion>>,
}

impl CompletionGuard {
    pub(crate) fn new(completion: UniquePtr<HttpCompletion>) -> Self {
        Self {
            completion: Some(completion),
        }
    }

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

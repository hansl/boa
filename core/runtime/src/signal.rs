//! Module implementing the `AbortSignal` and `AbortController`.
//!
//! See the [mdn documentation][<https://developer.mozilla.org/en-US/docs/Web/API/AbortController>]

use std::sync::atomic::Ordering;
use boa_engine::{Finalize, JsData, Trace, boa_class, JsValue, JsString};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct AbortSignal {
    #[unsafe_ignore_trace]
    aborted: Arc<AtomicBool>,
    reason: JsValue,
}

impl AbortSignal {
    /// Create a new `AbortSignal`. This can only be built in Rust.
    pub fn new(aborted: Arc<AtomicBool>) -> Self {
        Self { aborted, reason: JsValue::undefined() }
    }

    pub fn set_reason(&mut self, reason: JsValue) {
        self.reason = reason;
    }
}

#[boa_class]
impl AbortSignal {
    #[boa(getter)]
    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    #[boa(getter)]
    pub fn reason(&self) -> JsValue {
        self.reason.clone()
    }
}

#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct AbortController {
    signal: JsValue,
    #[unsafe_ignore_trace]
    aborted: Arc<AtomicBool>,
}

#[boa_class]
impl AbortController {
    /// The `AbortController()` constructor creates a new `AbortController` object instance.
    #[boa(constructor)]
    pub fn new() -> Self {
        Self {
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    ///
}

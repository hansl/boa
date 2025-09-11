use boa_engine::object::builtins::JsFunction;
use boa_engine::{Finalize, JsData, JsString, JsValue, Trace, boa_class};
use boa_gc::{Gc, GcRefCell};
use std::collections::HashMap;

/// The [`Event`] interface represents an event which takes place on an [`EventTarget`].
#[derive(Debug, Default, Trace, Finalize, JsData)]
pub struct Event {
    r#type: JsString,
    bubbles: bool,
    cancelable: bool,
    current_target: Option<JsValue>,
    composed: bool,
    default_prevented: bool,
    event_phase: EventPhase,
    is_trusted: bool,
    target: JsValue,
    timestamp: u64,
}

#[boa_class]
impl Event {
    /// Create a new `Event`.
    #[boa(constructor)]
    pub fn new(ty: JsString) -> Self {
        Self { r#type: ty }
    }

    #[boa(getter)]
    pub fn bubbles(&self) -> bool {
        false
    }
}

/// The [`EventTarget`] interface is implemented by objects that can receive events and
/// may have listeners for them. In other words, any target of events implements the
/// three methods associated with this interface.
#[derive(Debug, Default, Trace, Finalize, JsData)]
pub struct EventTarget {
    events: GcRefCell<HashMap<JsString, JsFunction>>,
}

#[boa_class]
impl EventTarget {
    /// Create a new `EventTarget`.
    #[boa(constructor)]
    pub fn new() -> Self {
        Default::default()
    }
}

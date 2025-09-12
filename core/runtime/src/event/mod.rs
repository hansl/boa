//! Module implementation JavaScript classes for the `Event` API.
//!
//! See <https://developer.mozilla.org/en-US/docs/Web/API/Event>.
use boa_engine::object::builtins::JsFunction;
use boa_engine::realm::Realm;
use boa_engine::value::{TryFromJs, TryIntoJs};
use boa_engine::{
    Context, Finalize, JsData, JsResult, JsString, JsValue, Trace, boa_class, boa_module, js_error,
};
use boa_gc::GcRefCell;
use std::collections::HashMap;
use std::fmt::Debug;

/// The `eventPhase` read-only property of the Event interface indicates which phase
/// of the event flow is currently being evaluated.
///
/// Please note that those are DOM-specific event phases, and no attempt is made
/// by this crate to assign semantics to these. The host application should set the
/// event phase of an event object to whichever corresponds closest to the real
/// semantic in the context of the application.
///
/// See the [mdn documentation].
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Event/eventPhase
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum EventPhase {
    /// The event is not being processed at this time.
    #[default]
    None = 0,

    /// The event is being propagated through the target's ancestor objects.
    CapturingPhase,

    /// The event has arrived at the event's target.
    AtTarget,

    /// The event is propagating back up through the target's ancestors in
    /// reverse order, starting with the parent, and eventually reaching the
    /// containing Window.
    BubblingPhase,

    /// A custom event phase that has no DOM-specific meaning.
    Custom,
}

impl TryFromJs for EventPhase {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value
            .as_i32()
            .ok_or_else(|| js_error!(TypeError: "Invalid EventPhase value"))?
        {
            0 => Ok(EventPhase::None),
            1 => Ok(EventPhase::CapturingPhase),
            2 => Ok(EventPhase::AtTarget),
            3 => Ok(EventPhase::BubblingPhase),
            _ => Err(js_error!("Invalid EventPhase value")),
        }
    }
}

impl TryIntoJs for EventPhase {
    fn try_into_js(&self, _context: &mut Context) -> JsResult<JsValue> {
        match self {
            EventPhase::None => Ok(JsValue::from(0)),
            EventPhase::CapturingPhase => Ok(JsValue::from(1)),
            EventPhase::AtTarget => Ok(JsValue::from(2)),
            EventPhase::BubblingPhase => Ok(JsValue::from(3)),
            EventPhase::Custom => Ok(JsValue::from(4)),
        }
    }
}

/// Event state value indicating whether the event handler requested to
/// stop propagation.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum EventPropagation {
    /// Allow propagation. This needs to be handled by the application
    /// as event propagation has no meaning outside of DOM.
    Allow,

    /// Stop propagation after handling propagation of events to the
    /// current target.
    Stop,

    /// Immediately stop propagation, including other listeners on the
    /// current target.
    ImmediatelyStop,
}

/// Constructor optional `options` argument.
#[derive(Debug, Trace, Finalize, JsData, TryFromJs)]
pub struct EventConstructorOptions {
    /// A boolean value indicating whether the event bubbles. The default is false.
    bubbles: Option<bool>,

    /// A boolean value indicating whether the event can be cancelled. The default is false.
    cancelable: Option<bool>,

    /// A boolean value indicating whether the event will trigger listeners outside
    /// of a shadow root (see Event.composed for more details). The default is false.
    composed: Option<bool>,
}

/// The [`Event`] interface represents an event which takes place on an [`EventTarget`].
#[derive(Debug, Trace, Finalize, JsData)]
#[allow(clippy::struct_excessive_bools)]
pub struct Event {
    r#type: JsString,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
    current_target: Option<JsValue>,
    default_prevented: bool,
    is_trusted: bool,
    #[unsafe_ignore_trace]
    phase: EventPhase,
    target: JsValue,
    time_stamp: u64,

    #[unsafe_ignore_trace]
    propagation: EventPropagation,
}

impl Event {
    /// Set the `isTrusted` value of this event to true.
    pub fn set_trusted(&mut self) {
        self.is_trusted = true;
    }

    /// Set the `currentTarget` value of this event.
    pub fn set_current_target(&mut self, target: JsValue) {
        self.current_target = Some(target);
    }

    /// Clear the `currentTarget` value.
    pub fn clear_current_target(&mut self) {
        self.current_target = None;
    }

    /// Gets the propagation state of this event.
    pub fn propagation(&self) -> EventPropagation {
        self.propagation
    }
}

#[boa_class]
impl Event {
    /// See [`EventPhase::None`].
    const NONE: EventPhase = EventPhase::None;

    /// See [`EventPhase::CapturingPhase`].
    const CAPTURING_PHASE: EventPhase = EventPhase::CapturingPhase;

    /// See [`EventPhase::AtTarget`].
    const AT_TARGET: EventPhase = EventPhase::AtTarget;

    /// See [`EventPhase::BubblingPhase`].
    const BUBBLING_PHASE: EventPhase = EventPhase::BubblingPhase;

    /// See [`EventPhase::Custom`].
    const CUSTOM: EventPhase = EventPhase::Custom;

    /// Create a new `Event`.
    #[boa(constructor)]
    pub fn new(
        ty: JsString,
        options: Option<EventConstructorOptions>,
        context: &mut Context,
    ) -> Self {
        let (bubbles, cancelable, composed) = options
            .map(|o| {
                (
                    o.bubbles.unwrap_or_default(),
                    o.cancelable.unwrap_or_default(),
                    o.composed.unwrap_or_default(),
                )
            })
            .unwrap_or_default();

        Self {
            r#type: ty,
            bubbles,
            cancelable,
            composed,
            current_target: None,
            default_prevented: false,
            is_trusted: false,
            phase: EventPhase::None,
            target: JsValue::null(),
            time_stamp: context.clock().now().millis_since_epoch(),
            propagation: EventPropagation::Allow,
        }
    }

    /// The `bubbles` read-only property of the `Event` interface indicates whether the
    /// event bubbles up through the DOM tree or not.
    #[boa(getter)]
    #[must_use]
    pub fn bubbles(&self) -> bool {
        self.bubbles
    }

    /// The `cancelable` read-only property of the `Event` interface indicates whether
    /// the event can be canceled and therefore prevented as if the event never
    /// happened.
    #[boa(getter)]
    #[must_use]
    pub fn cancelable(&self) -> bool {
        self.cancelable
    }

    /// The read-only `composed` property of the `Event` interface returns a boolean
    /// value which indicates whether or not the event will propagate across the
    /// shadow DOM boundary into the standard DOM.
    #[boa(getter)]
    #[must_use]
    pub fn composed(&self) -> bool {
        self.composed
    }

    /// The `currentTarget` read-only property of the `Event` interface identifies
    /// the element to which the event handler has been attached.
    #[boa(getter)]
    #[must_use]
    pub fn current_target(&self) -> JsValue {
        self.current_target.as_ref().unwrap_or(&self.target).clone()
    }

    /// The `defaultPrevented` read-only property of the `Event` interface returns
    /// a boolean value indicating whether or not the call to
    /// `Event.preventDefault()` canceled the event.
    #[boa(getter)]
    #[must_use]
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// The `eventPhase` read-only property of the `Event` interface indicates which
    /// phase of the event flow is currently being evaluated.
    #[boa(getter)]
    #[must_use]
    pub fn event_phase(&self) -> EventPhase {
        self.phase
    }

    /// The `isTrusted` read-only property of the `Event` interface is a boolean value
    /// that is `true` when the event was generated by the user agent (including via
    /// user actions and programmatic methods such as `HTMLElement.focus()`), and
    /// `false` when the event was dispatched via `EventTarget.dispatchEvent()`.
    /// The only exception is the `click` event, which initializes the `isTrusted`
    /// property to `false` in user agents.
    #[boa(getter)]
    #[must_use]
    pub fn is_trusted(&self) -> bool {
        self.is_trusted
    }

    /// The read-only `target` property of the `Event` interface is a reference to the
    /// object onto which the event was dispatched. It is different from
    /// `Event.currentTarget` when the event handler is called during the bubbling or
    /// capturing phase of the event.
    #[boa(getter)]
    #[must_use]
    pub fn target(&self) -> JsValue {
        self.target.clone()
    }

    /// The `timeStamp` read-only property of the `Event` interface returns the time
    /// (in milliseconds) at which the event was created.
    #[boa(getter)]
    #[must_use]
    pub fn time_stamp(&self) -> u64 {
        self.time_stamp
    }

    /// The `type` read-only property of the `Event` interface returns a string
    /// containing the event's type. It is set when the event is constructed and is
    /// the name commonly used to refer to the specific event, such as `click`,
    /// `load`, or `error`.
    #[boa(getter)]
    #[must_use]
    pub fn r#type(&self) -> JsString {
        self.r#type.clone()
    }

    /// The `composedPath()` method of the `Event` interface returns the event's
    /// path which is an array of the objects on which listeners will be invoked.
    ///
    /// # Errors
    /// This is currently unsupported by `boa_runtime`.
    pub fn composed_path(&self) -> JsResult<()> {
        Err(js_error!(Error: "Unsupported method 'composedPath'."))
    }

    /// The `preventDefault()` method of the `Event` interface tells the user
    /// agent that the event is being explicitly handled, so its default action,
    /// such as page scrolling, link navigation, or pasting text, should not be
    /// taken.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// The `stopImmediatePropagation()` method of the `Event` interface prevents
    /// other listeners of the same event from being called.
    pub fn stop_immediate_propagation(&mut self) {
        self.propagation = EventPropagation::ImmediatelyStop;
    }

    /// The `stopPropagation()` method of the `Event` interface prevents further
    /// propagation of the current event in the capturing and bubbling phases. It
    /// does not, however, prevent any default behaviors from occurring; for
    /// instance, clicks on links are still processed. If you want to stop those
    /// behaviors, see the `preventDefault()` method. It also does not prevent
    /// propagation to other event-handlers of the current element. If you want
    /// to stop those, see `stopImmediatePropagation()`.
    pub fn stop_propagation(&mut self) {
        if self.propagation == EventPropagation::Allow {
            self.propagation = EventPropagation::Stop;
        }
    }
}

#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct EventListener {
    handler: JsFunction,
    capture: bool,
    once: bool,
    passive: bool,
    signal: Option<JsValue>
}

/// The [`EventTarget`] interface is implemented by objects that can receive events and
/// may have listeners for them. In other words, any target of events implements the
/// three methods associated with this interface.
#[derive(Trace, Finalize, JsData)]
pub struct EventTarget {
    events: GcRefCell<HashMap<JsString, EventListener>>,
}

impl Default for EventTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for EventTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventTarget")
            .field("events", &self.events.borrow())
            .finish()
    }
}

#[boa_class]
impl EventTarget {
    /// Create a new `EventTarget`.
    #[boa(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: GcRefCell::new(HashMap::new()),
        }
    }

    /// The `addEventListener()` method of the `EventTarget` interface sets up a
    /// function that will be called whenever the specified event is delivered
    /// to the target.
    pub fn add_event_listener(&mut self, )
}

/// JavaScript module containing the event classes.
#[boa_module]
pub mod js_module {
    type Event = super::Event;
    type EventTarget = super::EventTarget;
}

/// Register the `Event`-related classes in the global context.
///
/// # Errors
/// Return an error if classes are already registered.
pub fn register(realm: Option<Realm>, context: &mut Context) -> JsResult<()> {
    js_module::boa_register(realm, context)
}

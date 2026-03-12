use crate::{
    Context, JsResult, JsString, JsSymbol, JsValue,
    object::{JsObject, PrivateName},
};
use boa_ast::scope::{BindingLocator, BindingLocatorScope, Scope};
use boa_gc::{Finalize, Gc, Trace, custom_trace};
use std::cell::Cell;
use thin_vec::ThinVec;

mod declarative;
mod private;

use self::declarative::ModuleEnvironment;
pub(crate) use self::{
    declarative::{
        DeclarativeEnvironment, DeclarativeEnvironmentKind, FunctionEnvironment, FunctionSlots,
        LexicalEnvironment, ThisBindingStatus,
    },
    private::PrivateEnvironment,
};

// ---------------------------------------------------------------------------
// Captured chain — the linked list inherited from closures.
// ---------------------------------------------------------------------------

/// A single node in the captured environment chain.
///
/// Each node holds one [`Environment`] and a pointer to its parent.
/// Nodes are immutable after creation — pushing creates a new tip node.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) struct EnvironmentNode {
    env: Environment,
    parent: Option<Gc<EnvironmentNode>>,
}

/// A runtime environment (used inside the captured chain and for promotion).
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) enum Environment {
    Declarative(Gc<DeclarativeEnvironment>),
    Object(JsObject),
}

impl Environment {
    /// Returns the declarative environment if it is one.
    pub(crate) const fn as_declarative(&self) -> Option<&Gc<DeclarativeEnvironment>> {
        match self {
            Self::Declarative(env) => Some(env),
            Self::Object(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Local environments — flat Vec, zero Gc allocation on push.
// ---------------------------------------------------------------------------

/// A local environment pushed during the current function's execution.
///
/// `Inline` variants store data directly on the Vec without any Gc allocation.
/// When a closure captures the scope, inline entries are **promoted** to
/// `Promoted(Gc<DeclarativeEnvironment>)` so the closure and the outer scope
/// share the same Gc-managed data.
pub(crate) enum LocalEnvironment {
    /// Bindings stored inline — no Gc allocation.
    Inline {
        kind: DeclarativeEnvironmentKind,
        poisoned: Cell<bool>,
        with: bool,
    },
    /// Promoted to heap after closure capture. Shared with closure.
    Promoted(Gc<DeclarativeEnvironment>),
    /// Object environment (for `with` statements).
    Object(JsObject),
    /// Sentinel used during promotion (replaced immediately).
    _Vacant,
}

impl LocalEnvironment {
    /// Promote this environment to a `Gc<DeclarativeEnvironment>` if it is inline.
    fn promote_if_inline(&mut self) {
        if matches!(self, Self::Inline { .. }) {
            let old = std::mem::replace(self, Self::_Vacant);
            if let Self::Inline {
                kind,
                poisoned,
                with,
            } = old
            {
                *self = Self::Promoted(Gc::new(DeclarativeEnvironment::new(
                    kind,
                    poisoned.get(),
                    with,
                )));
            }
        }
    }
}

// Manual Trace — must trace JsValues inside Inline kind, Gc in Promoted, JsObject in Object.
unsafe impl Trace for LocalEnvironment {
    custom_trace!(this, mark, {
        match this {
            Self::Inline { kind, .. } => mark(kind),
            Self::Promoted(gc) => mark(gc),
            Self::Object(obj) => mark(obj),
            Self::_Vacant => {}
        }
    });
}

impl Finalize for LocalEnvironment {}

impl Clone for LocalEnvironment {
    fn clone(&self) -> Self {
        match self {
            Self::Promoted(gc) => Self::Promoted(gc.clone()),
            Self::Object(obj) => Self::Object(obj.clone()),
            Self::Inline { .. } => {
                panic!("Cannot clone inline local environment; call promote_all() first")
            }
            Self::_Vacant => Self::_Vacant,
        }
    }
}

impl std::fmt::Debug for LocalEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { kind, poisoned, with } => f
                .debug_struct("Inline")
                .field("kind", kind)
                .field("poisoned", poisoned)
                .field("with", with)
                .finish(),
            Self::Promoted(gc) => f.debug_tuple("Promoted").field(gc).finish(),
            Self::Object(obj) => f.debug_tuple("Object").field(obj).finish(),
            Self::_Vacant => write!(f, "_Vacant"),
        }
    }
}

// ---------------------------------------------------------------------------
// EnvironmentStack
// ---------------------------------------------------------------------------

/// The environment stack holds all environments at runtime.
///
/// Split into two regions:
/// - **Captured**: a linked list of `Gc<EnvironmentNode>` inherited from the
///   closure chain (already heap-allocated, immutable structure).
/// - **Local**: a `Vec<LocalEnvironment>` for environments pushed during this
///   function's execution (zero Gc allocation for inline entries).
///
/// When a closure is created (`snapshot_for_closure`), inline locals are
/// promoted to Gc and linked into the captured chain. After promotion,
/// mutations are visible to both the outer scope and the closure.
///
/// The global declarative environment is NOT stored here — it lives in the
/// [`crate::realm::Realm`] and is accessed via `frame.realm.environment()`.
#[derive(Debug, Trace, Finalize)]
pub(crate) struct EnvironmentStack {
    /// The tip (most recently pushed) environment in the captured chain.
    captured_tip: Option<Gc<EnvironmentNode>>,

    /// Number of environments in the captured chain (not counting global).
    #[unsafe_ignore_trace]
    captured_depth: u32,

    /// Environments pushed during this function's execution (flat, typically no Gc).
    local: Vec<LocalEnvironment>,

    private_stack: ThinVec<Gc<PrivateEnvironment>>,
}

impl Clone for EnvironmentStack {
    fn clone(&self) -> Self {
        // Clone is used by:
        // - OrdinaryFunction::environments.clone() in function_call (local is always empty)
        // - Generator frame cloning (should promote_all first)
        // - Eval frame cloning (should promote_all first)
        // - Module frame cloning (should promote_all first)
        // Panics if any local is Inline (LocalEnvironment::clone panics).
        Self {
            captured_tip: self.captured_tip.clone(),
            captured_depth: self.captured_depth,
            local: self.local.clone(),
            private_stack: self.private_stack.clone(),
        }
    }
}

/// Saved environment state for `pop_to_global` / `restore_from_saved`.
/// Used by indirect `eval`.
pub(crate) struct SavedEnvironments {
    captured_tip: Option<Gc<EnvironmentNode>>,
    captured_depth: u32,
    local: Vec<LocalEnvironment>,
}

impl EnvironmentStack {
    /// Create a new environment stack.
    pub(crate) fn new() -> Self {
        Self {
            captured_tip: None,
            captured_depth: 0,
            local: Vec::new(),
            private_stack: ThinVec::new(),
        }
    }

    /// Get the total number of environments (captured + local, not counting global).
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.captured_depth as usize + self.local.len()
    }

    // ---- Push operations ----

    /// Push a lexical environment. **No Gc allocation.**
    pub(crate) fn push_lexical(
        &mut self,
        bindings_count: u32,
        global: &Gc<DeclarativeEnvironment>,
    ) -> u32 {
        let (poisoned, with) = self.compute_poisoned_with(global);
        let index = self.len() as u32;
        self.local.push(LocalEnvironment::Inline {
            kind: DeclarativeEnvironmentKind::Lexical(LexicalEnvironment::new(bindings_count)),
            poisoned: Cell::new(poisoned),
            with,
        });
        index
    }

    /// Push a function environment. **No Gc allocation.**
    pub(crate) fn push_function(
        &mut self,
        scope: Scope,
        function_slots: FunctionSlots,
        global: &Gc<DeclarativeEnvironment>,
    ) {
        let num_bindings = scope.num_bindings_non_local();
        let (poisoned, with) = self.compute_poisoned_with(global);
        self.local.push(LocalEnvironment::Inline {
            kind: DeclarativeEnvironmentKind::Function(FunctionEnvironment::new(
                num_bindings,
                function_slots,
                scope,
            )),
            poisoned: Cell::new(poisoned),
            with,
        });
    }

    /// Push a module environment. Module envs are always promoted immediately
    /// because they're global singletons referenced across modules.
    pub(crate) fn push_module(&mut self, scope: Scope) {
        let num_bindings = scope.num_bindings_non_local();
        self.local.push(LocalEnvironment::Promoted(Gc::new(
            DeclarativeEnvironment::new(
                DeclarativeEnvironmentKind::Module(ModuleEnvironment::new(num_bindings, scope)),
                false,
                false,
            ),
        )));
    }

    /// Push an object environment on the stack.
    pub(crate) fn push_object(&mut self, object: JsObject) {
        self.local.push(LocalEnvironment::Object(object));
    }

    // ---- Pop / Truncate ----

    /// Pop the most recently pushed environment.
    #[track_caller]
    pub(crate) fn pop(&mut self) {
        if self.local.pop().is_some() {
            return;
        }
        // Fall back to captured chain (shouldn't happen in normal execution).
        let node = self
            .captured_tip
            .as_ref()
            .expect("cannot pop empty environment chain");
        self.captured_tip = node.parent.clone();
        self.captured_depth -= 1;
    }

    /// Truncate to the given total depth.
    pub(crate) fn truncate(&mut self, len: usize) {
        let captured = self.captured_depth as usize;
        if len >= captured {
            self.local.truncate(len - captured);
        } else {
            self.local.clear();
            while self.captured_depth as usize > len {
                let node = self
                    .captured_tip
                    .as_ref()
                    .expect("depth > 0 implies tip is Some");
                self.captured_tip = node.parent.clone();
                self.captured_depth -= 1;
            }
        }
    }

    /// Save all environments and clear the stack. Used by indirect eval.
    pub(crate) fn pop_to_global(&mut self) -> SavedEnvironments {
        SavedEnvironments {
            captured_tip: self.captured_tip.take(),
            captured_depth: std::mem::replace(&mut self.captured_depth, 0),
            local: std::mem::take(&mut self.local),
        }
    }

    /// Restore environments from a previous `pop_to_global` call.
    pub(crate) fn restore_from_saved(&mut self, saved: SavedEnvironments) {
        self.captured_tip = saved.captured_tip;
        self.captured_depth = saved.captured_depth;
        self.local = saved.local;
    }

    // ---- Binding access (hot path) ----

    /// Get a binding value from a declarative environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the environment is an object environment or indices are out of range.
    #[track_caller]
    pub(crate) fn get_binding_value(&self, env_index: u32, binding_index: u32) -> Option<JsValue> {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { kind, .. } => kind.get(binding_index),
                LocalEnvironment::Promoted(gc) => gc.get(binding_index),
                LocalEnvironment::Object(_) | LocalEnvironment::_Vacant => {
                    panic!("expected declarative environment at index {env_index}")
                }
            }
        } else {
            let env = self
                .captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment in captured chain");
            env.get(binding_index)
        }
    }

    /// Set a binding value in a declarative environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the environment is an object environment or indices are out of range.
    #[track_caller]
    pub(crate) fn set_binding_value(&self, env_index: u32, binding_index: u32, value: JsValue) {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { kind, .. } => kind.set(binding_index, value),
                LocalEnvironment::Promoted(gc) => gc.set(binding_index, value),
                LocalEnvironment::Object(_) | LocalEnvironment::_Vacant => {
                    panic!("expected declarative environment at index {env_index}")
                }
            }
        } else {
            let env = self
                .captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment in captured chain");
            env.set(binding_index, value);
        }
    }

    /// Check if the environment at the given index is an object environment.
    pub(crate) fn is_object_env(&self, env_index: u32) -> bool {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            matches!(&self.local[local_idx], LocalEnvironment::Object(_))
        } else {
            matches!(self.captured_get(env_index), Some(Environment::Object(_)))
        }
    }

    /// Get the object from an object environment.
    pub(crate) fn get_object_env(&self, env_index: u32) -> Option<&JsObject> {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Object(obj) => Some(obj),
                _ => None,
            }
        } else {
            match self.captured_get(env_index)? {
                Environment::Object(obj) => Some(obj),
                Environment::Declarative(_) => None,
            }
        }
    }

    /// Get the `poisoned` flag of a declarative environment.
    pub(crate) fn env_poisoned(&self, env_index: u32) -> bool {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { poisoned, .. } => poisoned.get(),
                LocalEnvironment::Promoted(gc) => gc.poisoned(),
                _ => panic!("expected declarative environment"),
            }
        } else {
            self.captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment")
                .poisoned()
        }
    }

    /// Get the `with` flag of a declarative environment.
    pub(crate) fn env_with(&self, env_index: u32) -> bool {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { with, .. } => *with,
                LocalEnvironment::Promoted(gc) => gc.with(),
                _ => panic!("expected declarative environment"),
            }
        } else {
            self.captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment")
                .with()
        }
    }

    /// Get the kind of a declarative environment at the given index.
    pub(crate) fn env_kind(&self, env_index: u32) -> &DeclarativeEnvironmentKind {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { kind, .. } => kind,
                LocalEnvironment::Promoted(gc) => gc.kind(),
                _ => panic!("expected declarative environment"),
            }
        } else {
            self.captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment")
                .kind()
        }
    }

    /// Poison a declarative environment at the given index.
    #[allow(dead_code)]
    pub(crate) fn env_poison(&self, env_index: u32) {
        let captured = self.captured_depth;
        if env_index >= captured {
            let local_idx = (env_index - captured) as usize;
            match &self.local[local_idx] {
                LocalEnvironment::Inline { poisoned, .. } => poisoned.set(true),
                LocalEnvironment::Promoted(gc) => gc.poison(),
                _ => panic!("expected declarative environment"),
            }
        } else {
            self.captured_get(env_index)
                .and_then(Environment::as_declarative)
                .expect("expected declarative environment")
                .poison();
        }
    }

    /// Check if the declarative environment at the given index is a function env.
    #[allow(dead_code)]
    pub(crate) fn env_is_function(&self, env_index: u32) -> bool {
        matches!(self.env_kind(env_index), DeclarativeEnvironmentKind::Function(_))
    }

    // ---- Tip (current environment) access ----

    /// Returns `true` if the tip environment is a non-poisoned, non-with declarative env.
    /// Used as the fast-path early exit in `find_runtime_binding`.
    pub(crate) fn current_is_clean_declarative(
        &self,
        global: &Gc<DeclarativeEnvironment>,
    ) -> bool {
        if let Some(local) = self.local.last() {
            match local {
                LocalEnvironment::Inline { poisoned, with, .. } => !poisoned.get() && !*with,
                LocalEnvironment::Promoted(gc) => !gc.poisoned() && !gc.with(),
                LocalEnvironment::Object(_) | LocalEnvironment::_Vacant => false,
            }
        } else if let Some(node) = &self.captured_tip {
            match &node.env {
                Environment::Declarative(gc) => !gc.poisoned() && !gc.with(),
                Environment::Object(_) => false,
            }
        } else {
            // Empty stack — check global.
            !global.with() && !global.poisoned()
        }
    }

    /// Returns `true` if the tip is a declarative env that is not a `with` wrapper.
    /// Used as the fast-path early exit in `this_from_object_environment_binding`.
    pub(crate) fn current_is_not_with(
        &self,
        global: &Gc<DeclarativeEnvironment>,
    ) -> bool {
        if let Some(local) = self.local.last() {
            match local {
                LocalEnvironment::Inline { with, .. } => !*with,
                LocalEnvironment::Promoted(gc) => !gc.with(),
                LocalEnvironment::Object(_) | LocalEnvironment::_Vacant => false,
            }
        } else if let Some(node) = &self.captured_tip {
            match &node.env {
                Environment::Declarative(gc) => !gc.with(),
                Environment::Object(_) => false,
            }
        } else {
            !global.with()
        }
    }

    /// Get the tip environment's `DeclarativeEnvironmentKind`.
    /// Falls back to the global environment if the stack is empty.
    /// Panics if the tip is an object environment.
    pub(crate) fn current_declarative_kind<'a>(
        &'a self,
        global: &'a Gc<DeclarativeEnvironment>,
    ) -> &'a DeclarativeEnvironmentKind {
        if let Some(local) = self.local.last() {
            match local {
                LocalEnvironment::Inline { kind, .. } => kind,
                LocalEnvironment::Promoted(gc) => gc.kind(),
                _ => panic!("expected declarative environment at tip"),
            }
        } else if let Some(node) = &self.captured_tip {
            match &node.env {
                Environment::Declarative(gc) => gc.kind(),
                Environment::Object(_) => panic!("expected declarative environment at tip"),
            }
        } else {
            global.kind()
        }
    }

    /// Get the tip declarative environment as a `Gc<DeclarativeEnvironment>`.
    /// Force-promotes the tip if it is inline.
    /// Returns the global if the stack is empty.
    /// Returns `None` if the tip is an object environment.
    pub(crate) fn current_declarative_gc(
        &mut self,
        global: &Gc<DeclarativeEnvironment>,
    ) -> Option<Gc<DeclarativeEnvironment>> {
        if let Some(local) = self.local.last_mut() {
            local.promote_if_inline();
            match local {
                LocalEnvironment::Promoted(gc) => Some(gc.clone()),
                LocalEnvironment::Object(_) => None,
                _ => unreachable!(),
            }
        } else if let Some(node) = &self.captured_tip {
            node.env.as_declarative().cloned()
        } else {
            Some(global.clone())
        }
    }

    /// Get the tip declarative environment as a `&Gc<DeclarativeEnvironment>`.
    /// Only works if the tip is already a Gc (Promoted, captured, or global).
    /// Returns `None` if the tip is inline or object.
    pub(crate) fn current_declarative_ref<'a>(
        &'a self,
        global: &'a Gc<DeclarativeEnvironment>,
    ) -> Option<&'a Gc<DeclarativeEnvironment>> {
        if let Some(local) = self.local.last() {
            match local {
                LocalEnvironment::Promoted(gc) => Some(gc),
                _ => None,
            }
        } else if let Some(node) = &self.captured_tip {
            node.env.as_declarative()
        } else {
            Some(global)
        }
    }

    // ---- This environment ----

    /// `GetThisEnvironment`
    ///
    /// Returns the environment that currently provides a `this` binding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-getthisenvironment
    pub(crate) fn get_this_environment<'a>(
        &'a self,
        global: &'a Gc<DeclarativeEnvironment>,
    ) -> &'a DeclarativeEnvironmentKind {
        // Search local environments (most recent first).
        for local in self.local.iter().rev() {
            match local {
                LocalEnvironment::Inline { kind, .. } if kind.has_this_binding() => {
                    return kind;
                }
                LocalEnvironment::Promoted(gc) if gc.has_this_binding() => {
                    return gc.kind();
                }
                _ => {}
            }
        }
        // Search captured chain.
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if let Environment::Declarative(gc) = &node.env
                && gc.has_this_binding() {
                    return gc.kind();
                }
            current = node.parent.as_deref();
        }
        // Fall back to global.
        global.kind()
    }

    /// `GetThisBinding`
    ///
    /// Returns the current `this` binding.
    /// Returns `Ok(None)` if the current environment is the global environment.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-getthisbinding
    pub(crate) fn get_this_binding(&self) -> JsResult<Option<JsValue>> {
        // Search local environments.
        for local in self.local.iter().rev() {
            match local {
                LocalEnvironment::Inline { kind, .. } => {
                    if let Some(this) = kind.get_this_binding()? {
                        return Ok(Some(this));
                    }
                }
                LocalEnvironment::Promoted(gc) => {
                    if let Some(this) = gc.get_this_binding()? {
                        return Ok(Some(this));
                    }
                }
                _ => {}
            }
        }
        // Search captured chain.
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if let Environment::Declarative(gc) = &node.env
                && let Some(this) = gc.get_this_binding()? {
                    return Ok(Some(this));
                }
            current = node.parent.as_deref();
        }
        Ok(None)
    }

    // ---- Outer function environment ----

    /// Gets the next outer function environment.
    #[allow(dead_code)]
    pub(crate) fn outer_function_environment(
        &self,
    ) -> Option<(Gc<DeclarativeEnvironment>, Scope)> {
        // Search local environments.
        for local in self.local.iter().rev() {
            let kind = match local {
                LocalEnvironment::Inline { kind, .. } => kind,
                LocalEnvironment::Promoted(gc) => gc.kind(),
                _ => continue,
            };
            if let Some(func_env) = kind.as_function() {
                // For inline envs we need a Gc — but outer_function_environment
                // is only called by eval (slow path), so an allocation here is fine.
                // We don't force-promote here because we only have &self.
                // Instead, we create a temporary Gc for the return value.
                // Actually, callers need a Gc<DeclarativeEnvironment>, so we must
                // force-promote. But we only have &self... Let's handle this via
                // a separate _mut version or by searching captured instead.
                //
                // In practice, when eval calls this, the function env was pushed
                // before any eval code ran. If push_function is now Inline, we
                // need &mut self to promote. Let's look in the captured chain too.
                //
                // Actually this is simpler: the function calling eval pushed its
                // function env. That env is inline. We need to return a
                // Gc<DeclarativeEnvironment> for it. Since we only have &self,
                // we can't promote. But wait — does eval actually need the Gc
                // for extend_from_compile? Yes. So we need a &mut self version.
                // Let's search captured chain to see if the function env is there.
                let _ = func_env;
                break; // Fall through to captured search or use _mut version
            }
        }
        // Search captured chain (function envs from closures are here).
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if let Environment::Declarative(gc) = &node.env
                && let Some(func_env) = gc.kind().as_function() {
                    return Some((gc.clone(), func_env.compile().clone()));
                }
            current = node.parent.as_deref();
        }
        None
    }

    /// Gets the next outer function environment, promoting inline envs if needed.
    /// Only called by eval (slow path).
    pub(crate) fn outer_function_environment_mut(
        &mut self,
    ) -> Option<(Gc<DeclarativeEnvironment>, Scope)> {
        // Search local environments — promote inline function env if found.
        for local in self.local.iter_mut().rev() {
            match local {
                LocalEnvironment::Inline { kind, .. } => {
                    if kind.as_function().is_some() {
                        local.promote_if_inline();
                        if let LocalEnvironment::Promoted(gc) = local {
                            let scope = gc
                                .kind()
                                .as_function()
                                .expect("just promoted a function env")
                                .compile()
                                .clone();
                            return Some((gc.clone(), scope));
                        }
                    }
                }
                LocalEnvironment::Promoted(gc) => {
                    if let Some(func_env) = gc.kind().as_function() {
                        return Some((gc.clone(), func_env.compile().clone()));
                    }
                }
                _ => {}
            }
        }
        // Search captured chain.
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if let Environment::Declarative(gc) = &node.env
                && let Some(func_env) = gc.kind().as_function() {
                    return Some((gc.clone(), func_env.compile().clone()));
                }
            current = node.parent.as_deref();
        }
        None
    }

    // ---- Poisoning ----

    /// Mark that there may be added bindings from the current environment to the
    /// next function environment.
    pub(crate) fn poison_until_last_function(&mut self, global: &Gc<DeclarativeEnvironment>) {
        // Poison local environments from tip toward base.
        for local in self.local.iter().rev() {
            match local {
                LocalEnvironment::Inline { poisoned, kind, .. } => {
                    poisoned.set(true);
                    if kind.as_function().is_some() {
                        return;
                    }
                }
                LocalEnvironment::Promoted(gc) => {
                    gc.poison();
                    if gc.is_function() {
                        return;
                    }
                }
                _ => {}
            }
        }
        // Continue into captured chain.
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if let Environment::Declarative(gc) = &node.env {
                gc.poison();
                if gc.is_function() {
                    return;
                }
            }
            current = node.parent.as_deref();
        }
        global.poison();
    }

    /// Indicate if the environment stack has any object environment.
    pub(crate) fn has_object_environment(&self) -> bool {
        for local in self.local.iter().rev() {
            if matches!(local, LocalEnvironment::Object(_)) {
                return true;
            }
        }
        let mut current = self.captured_tip.as_deref();
        while let Some(node) = current {
            if matches!(&node.env, Environment::Object(_)) {
                return true;
            }
            current = node.parent.as_deref();
        }
        false
    }

    // ---- Lexical value operations ----

    /// Set the value of a lexical binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn put_lexical_value(
        &mut self,
        environment: BindingLocatorScope,
        binding_index: u32,
        value: JsValue,
        global: &Gc<DeclarativeEnvironment>,
    ) {
        match environment {
            BindingLocatorScope::GlobalObject | BindingLocatorScope::GlobalDeclarative => {
                global.set(binding_index, value);
            }
            BindingLocatorScope::Stack(index) => {
                self.set_binding_value(index, binding_index, value);
            }
        }
    }

    /// Set the value of a binding if it is uninitialized.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn put_value_if_uninitialized(
        &mut self,
        environment: BindingLocatorScope,
        binding_index: u32,
        value: JsValue,
        global: &Gc<DeclarativeEnvironment>,
    ) {
        match environment {
            BindingLocatorScope::GlobalObject | BindingLocatorScope::GlobalDeclarative => {
                if global.get(binding_index).is_none() {
                    global.set(binding_index, value);
                }
            }
            BindingLocatorScope::Stack(index) => {
                if self.get_binding_value(index, binding_index).is_none() {
                    self.set_binding_value(index, binding_index, value);
                }
            }
        }
    }

    // ---- Snapshot / Promote ----

    /// Promote all inline local environments to Gc-managed.
    /// Must be called before `clone()` or any other operation that needs Gc references.
    pub(crate) fn promote_all(&mut self) {
        for local in &mut self.local {
            local.promote_if_inline();
        }
    }

    /// Create an `EnvironmentStack` snapshot for storing in a closure.
    ///
    /// Promotes all inline locals, chains them into the captured linked list,
    /// and returns a new stack with everything in the captured region and an
    /// empty local region.
    pub(crate) fn snapshot_for_closure(&mut self) -> EnvironmentStack {
        self.promote_all();

        // Build chain: captured_tip ← local[0] ← local[1] ← ... ← local[n-1] (new tip)
        let mut tip = self.captured_tip.clone();
        for local in &self.local {
            let env = match local {
                LocalEnvironment::Promoted(gc) => Environment::Declarative(gc.clone()),
                LocalEnvironment::Object(obj) => Environment::Object(obj.clone()),
                LocalEnvironment::_Vacant => continue,
                LocalEnvironment::Inline { .. } => unreachable!("all promoted"),
            };
            tip = Some(Gc::new(EnvironmentNode { env, parent: tip }));
        }

        EnvironmentStack {
            captured_tip: tip,
            captured_depth: self.captured_depth + self.local.len() as u32,
            local: Vec::new(),
            private_stack: self.private_stack.clone(),
        }
    }

    // ---- Private environments ----

    /// Push a private environment.
    pub(crate) fn push_private(&mut self, environment: Gc<PrivateEnvironment>) {
        self.private_stack.push(environment);
    }

    /// Pop a private environment.
    pub(crate) fn pop_private(&mut self) {
        self.private_stack.pop();
    }

    /// `ResolvePrivateIdentifier ( privEnv, identifier )`
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-resolve-private-identifier
    pub(crate) fn resolve_private_identifier(&self, identifier: JsString) -> Option<PrivateName> {
        for environment in self.private_stack.iter().rev() {
            if environment.descriptions().contains(&identifier) {
                return Some(PrivateName::new(identifier, environment.id()));
            }
        }
        None
    }

    /// Return all private name descriptions in all private environments.
    pub(crate) fn private_name_descriptions(&self) -> Vec<&JsString> {
        let mut names = Vec::new();
        for environment in self.private_stack.iter().rev() {
            for name in environment.descriptions() {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    // ---- Private helpers ----

    /// Get an environment from the captured chain by absolute index (0-based from root).
    fn captured_get(&self, index: u32) -> Option<&Environment> {
        debug_assert!(
            index < self.captured_depth,
            "captured_get index out of range"
        );
        let steps = self.captured_depth - 1 - index;
        let mut current = self.captured_tip.as_deref()?;
        for _ in 0..steps {
            current = current.parent.as_deref()?;
        }
        Some(&current.env)
    }

    /// Compute the `(poisoned, with)` flags for a new environment.
    fn compute_poisoned_with(&self, global: &Gc<DeclarativeEnvironment>) -> (bool, bool) {
        // `with` is true if the tip is an object environment.
        // `poisoned` inherits from the nearest declarative environment.
        if let Some(local) = self.local.last() {
            match local {
                LocalEnvironment::Inline {
                    poisoned, with, ..
                } => (poisoned.get(), *with),
                LocalEnvironment::Promoted(gc) => (gc.poisoned(), gc.with()),
                LocalEnvironment::Object(_) => {
                    // Object env at tip → new env is `with: true`.
                    // Find nearest declarative for poisoned.
                    let poisoned = self
                        .local
                        .iter()
                        .rev()
                        .skip(1)
                        .find_map(|l| match l {
                            LocalEnvironment::Inline { poisoned, .. } => Some(poisoned.get()),
                            LocalEnvironment::Promoted(gc) => Some(gc.poisoned()),
                            _ => None,
                        })
                        .unwrap_or_else(|| {
                            self.captured_iter()
                                .find_map(|env| env.as_declarative().map(|gc| gc.poisoned()))
                                .unwrap_or(global.poisoned())
                        });
                    (poisoned, true)
                }
                LocalEnvironment::_Vacant => (false, false),
            }
        } else if let Some(node) = &self.captured_tip {
            match &node.env {
                Environment::Declarative(gc) => (gc.poisoned(), gc.with()),
                Environment::Object(_) => {
                    let poisoned = self
                        .captured_iter()
                        .skip(1)
                        .find_map(|env| env.as_declarative().map(|gc| gc.poisoned()))
                        .unwrap_or(global.poisoned());
                    (poisoned, true)
                }
            }
        } else {
            (global.poisoned(), false)
        }
    }

    /// Iterate the captured chain from tip toward root, yielding `&Environment`.
    fn captured_iter(&self) -> CapturedIter<'_> {
        CapturedIter {
            current: self.captured_tip.as_deref(),
        }
    }
}

/// Iterator over captured chain environments from tip to root.
struct CapturedIter<'a> {
    current: Option<&'a EnvironmentNode>,
}

impl<'a> Iterator for CapturedIter<'a> {
    type Item = &'a Environment;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.current?;
        self.current = node.parent.as_deref();
        Some(&node.env)
    }
}

// ---------------------------------------------------------------------------
// Context methods — binding access
// ---------------------------------------------------------------------------

impl Context {
    /// Gets the corresponding runtime binding of the provided `BindingLocator`,
    /// modifying its indexes in place.
    ///
    /// This readjusts a `BindingLocator` to the correct binding if a `with`
    /// environment or `eval` call modified the compile-time bindings.
    pub(crate) fn find_runtime_binding(&mut self, locator: &mut BindingLocator) -> JsResult<()> {
        let global = self.vm.frame().realm.environment();
        if self.vm.frame().environments.current_is_clean_declarative(global) {
            return Ok(());
        }

        let (global, min_index) = match locator.scope() {
            BindingLocatorScope::GlobalObject | BindingLocatorScope::GlobalDeclarative => (true, 0),
            BindingLocatorScope::Stack(index) => (false, index),
        };
        let max_index = self.vm.frame().environments.len() as u32;

        for index in (min_index..max_index).rev() {
            if self.vm.frame().environments.is_object_env(index) {
                let o = self
                    .vm
                    .frame()
                    .environments
                    .get_object_env(index)
                    .expect("checked is_object_env")
                    .clone();
                let key = locator.name().clone();
                if o.has_property(key.clone(), self)? {
                    if let Some(unscopables) = o.get(JsSymbol::unscopables(), self)?.as_object()
                        && unscopables.get(key.clone(), self)?.to_boolean()
                    {
                        continue;
                    }
                    locator.set_scope(BindingLocatorScope::Stack(index));
                    return Ok(());
                }
            } else {
                let poisoned = self.vm.frame().environments.env_poisoned(index);
                if poisoned {
                    if let Some(func_env) = self.vm.frame().environments.env_kind(index).as_function()
                        && let Some(b) = func_env.compile().get_binding(locator.name())
                    {
                        locator.set_scope(b.scope());
                        locator.set_binding_index(b.binding_index());
                        return Ok(());
                    }
                } else if !self.vm.frame().environments.env_with(index) {
                    return Ok(());
                }
            }
        }

        if global
            && self.realm().environment().poisoned()
            && let Some(b) = self.realm().scope().get_binding(locator.name())
        {
            locator.set_scope(b.scope());
            locator.set_binding_index(b.binding_index());
        }

        Ok(())
    }

    /// Finds the object environment that contains the binding and returns the
    /// `this` value of the object environment.
    pub(crate) fn this_from_object_environment_binding(
        &mut self,
        locator: &BindingLocator,
    ) -> JsResult<Option<JsObject>> {
        let global = self.vm.frame().realm.environment();
        if self.vm.frame().environments.current_is_not_with(global) {
            return Ok(None);
        }

        let min_index = match locator.scope() {
            BindingLocatorScope::GlobalObject | BindingLocatorScope::GlobalDeclarative => 0,
            BindingLocatorScope::Stack(index) => index,
        };
        let max_index = self.vm.frame().environments.len() as u32;

        for index in (min_index..max_index).rev() {
            if self.vm.frame().environments.is_object_env(index) {
                let o = self
                    .vm
                    .frame()
                    .environments
                    .get_object_env(index)
                    .expect("checked is_object_env")
                    .clone();
                let key = locator.name().clone();
                if o.has_property(key.clone(), self)? {
                    if let Some(unscopables) = o.get(JsSymbol::unscopables(), self)?.as_object()
                        && unscopables.get(key.clone(), self)?.to_boolean()
                    {
                        continue;
                    }
                    return Ok(Some(o));
                }
            } else {
                let poisoned = self.vm.frame().environments.env_poisoned(index);
                if poisoned {
                    if let Some(func_env) = self.vm.frame().environments.env_kind(index).as_function()
                        && func_env.compile().get_binding(locator.name()).is_some()
                    {
                        break;
                    }
                } else if !self.vm.frame().environments.env_with(index) {
                    break;
                }
            }
        }

        Ok(None)
    }

    /// Checks if the binding pointed by `locator` is initialized.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    pub(crate) fn is_initialized_binding(&mut self, locator: &BindingLocator) -> JsResult<bool> {
        match locator.scope() {
            BindingLocatorScope::GlobalObject => {
                let key = locator.name().clone();
                let obj = self.global_object();
                obj.has_property(key, self)
            }
            BindingLocatorScope::GlobalDeclarative => {
                let env = self.vm.frame().realm.environment();
                Ok(env.get(locator.binding_index()).is_some())
            }
            BindingLocatorScope::Stack(index) => {
                if self.vm.frame().environments.is_object_env(index) {
                    let obj = self
                        .vm
                        .frame()
                        .environments
                        .get_object_env(index)
                        .expect("checked is_object_env")
                        .clone();
                    let key = locator.name().clone();
                    obj.has_property(key, self)
                } else {
                    Ok(self
                        .vm
                        .frame()
                        .environments
                        .get_binding_value(index, locator.binding_index())
                        .is_some())
                }
            }
        }
    }

    /// Get the value of a binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn get_binding(&mut self, locator: &BindingLocator) -> JsResult<Option<JsValue>> {
        match locator.scope() {
            BindingLocatorScope::GlobalObject => {
                let key = locator.name().clone();
                let obj = self.global_object();
                obj.try_get(key, self)
            }
            BindingLocatorScope::GlobalDeclarative => {
                let env = self.vm.frame().realm.environment();
                Ok(env.get(locator.binding_index()))
            }
            BindingLocatorScope::Stack(index) => {
                if self.vm.frame().environments.is_object_env(index) {
                    let obj = self
                        .vm
                        .frame()
                        .environments
                        .get_object_env(index)
                        .expect("checked is_object_env")
                        .clone();
                    let key = locator.name().clone();
                    obj.get(key, self).map(Some)
                } else {
                    Ok(self
                        .vm
                        .frame()
                        .environments
                        .get_binding_value(index, locator.binding_index()))
                }
            }
        }
    }

    /// Sets the value of a binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn set_binding(
        &mut self,
        locator: &BindingLocator,
        value: JsValue,
        strict: bool,
    ) -> JsResult<()> {
        match locator.scope() {
            BindingLocatorScope::GlobalObject => {
                let key = locator.name().clone();
                let obj = self.global_object();
                obj.set(key, value, strict, self)?;
            }
            BindingLocatorScope::GlobalDeclarative => {
                let env = self.vm.frame().realm.environment();
                env.set(locator.binding_index(), value);
            }
            BindingLocatorScope::Stack(index) => {
                if self.vm.frame().environments.is_object_env(index) {
                    let obj = self
                        .vm
                        .frame()
                        .environments
                        .get_object_env(index)
                        .expect("checked is_object_env")
                        .clone();
                    let key = locator.name().clone();
                    obj.set(key, value, strict, self)?;
                } else {
                    self.vm
                        .frame()
                        .environments
                        .set_binding_value(index, locator.binding_index(), value);
                }
            }
        }
        Ok(())
    }

    /// Deletes a binding if it exists.
    ///
    /// Returns `true` if the binding was deleted.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    pub(crate) fn delete_binding(&mut self, locator: &BindingLocator) -> JsResult<bool> {
        match locator.scope() {
            BindingLocatorScope::GlobalObject => {
                let key = locator.name().clone();
                let obj = self.global_object();
                obj.__delete__(&key.into(), &mut self.into())
            }
            BindingLocatorScope::GlobalDeclarative => Ok(false),
            BindingLocatorScope::Stack(index) => {
                if self.vm.frame().environments.is_object_env(index) {
                    let obj = self
                        .vm
                        .frame()
                        .environments
                        .get_object_env(index)
                        .expect("checked is_object_env")
                        .clone();
                    let key = locator.name().clone();
                    obj.__delete__(&key.into(), &mut self.into())
                } else {
                    Ok(false)
                }
            }
        }
    }
}

mod function;
mod global;
mod lexical;
mod module;

pub(crate) use function::{FunctionEnvironment, FunctionSlots, ThisBindingStatus};
pub(crate) use global::GlobalEnvironment;
pub(crate) use lexical::LexicalEnvironment;
pub(crate) use module::ModuleEnvironment;

use crate::{JsResult, JsValue};
use boa_gc::{Finalize, GcRefCell, Trace};
use std::cell::Cell;

/// A declarative environment holds binding values at runtime.
///
/// Bindings are stored in a fixed size list of optional values.
/// If a binding is not initialized, the value is `None`.
///
/// Optionally, an environment can hold a `this` value.
/// The `this` value is present only if the environment is a function environment.
///
/// Code evaluation at runtime (e.g. the `eval` built-in function) can add
/// bindings to existing, compiled function environments.
/// This makes it impossible to determine the location of all bindings at compile time.
/// To dynamically check for added bindings at runtime, a reference to the
/// corresponding compile time environment is needed.
///
/// Checking all environments for potential added bindings at runtime on every get/set
/// would offset the performance improvement of determining binding locations at compile time.
/// To minimize this, each environment holds a `poisoned` flag.
/// If bindings where added at runtime, the current environment and all inner environments
/// are marked as poisoned.
/// All poisoned environments have to be checked for added bindings.
#[derive(Debug, Trace, Finalize)]
pub(crate) struct DeclarativeEnvironment {
    kind: DeclarativeEnvironmentKind,
}

impl DeclarativeEnvironment {
    /// Creates a new global `DeclarativeEnvironment`.
    pub(crate) fn global() -> Self {
        Self {
            kind: DeclarativeEnvironmentKind::Global(GlobalEnvironment::new()),
        }
    }

    /// Creates a new `DeclarativeEnvironment` from its kind and compile environment.
    pub(crate) fn new(kind: DeclarativeEnvironmentKind) -> Self {
        Self { kind }
    }

    /// Returns a reference to the the kind of the environment.
    pub(crate) const fn kind(&self) -> &DeclarativeEnvironmentKind {
        &self.kind
    }

    /// Returns whether this environment is a function environment.
    pub(crate) fn is_function(&self) -> bool {
        matches!(self.kind(), DeclarativeEnvironmentKind::Function(_))
    }

    /// Gets the binding value from the environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range or not initialized.
    #[track_caller]
    pub(crate) fn get(&self, index: u32) -> Option<JsValue> {
        self.kind.get(index)
    }

    /// Sets the binding value from the environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range.
    #[track_caller]
    pub(crate) fn set(&self, index: u32, value: JsValue) {
        self.kind.set(index, value);
    }

    /// `GetThisBinding`
    ///
    /// Returns the `this` binding of this environment.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-getthisbinding
    pub(crate) fn get_this_binding(&self) -> JsResult<Option<JsValue>> {
        self.kind.get_this_binding()
    }

    /// `HasThisBinding`
    ///
    /// Returns if the environment has a `this` binding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-hasthisbinding
    pub(crate) fn has_this_binding(&self) -> bool {
        self.kind.has_this_binding()
    }

    /// Returns `true` if this environment is poisoned.
    pub(crate) fn poisoned(&self) -> bool {
        self.kind.poisoned()
    }

    /// Returns `true` if this environment is inside a `with` environment.
    pub(crate) fn with(&self) -> bool {
        self.kind.with()
    }

    /// Poisons this environment for future binding searchs.
    pub(crate) fn poison(&self) {
        self.kind.poison();
    }

    /// Extends the environment with the bindings from the compile time environment.
    pub(crate) fn extend_from_compile(&self) {
        if let Some(env) = self.kind().as_function() {
            let compile_bindings_number = env.compile().num_bindings() as usize;
            let mut bindings = env.poisonable_environment().bindings().borrow_mut();
            if compile_bindings_number > bindings.len() {
                bindings.resize(compile_bindings_number, None);
            }
        }
    }
}

/// The kind of the declarative environment.
#[derive(Debug, Trace, Finalize)]
pub(crate) enum DeclarativeEnvironmentKind {
    /// Only stores lexical bindings.
    Lexical(LexicalEnvironment),
    /// Stores lexical bindings, global var bindings and the global this.
    Global(GlobalEnvironment),
    /// Stores lexical bindings, var bindings and the `FunctionSlots` of the function environment.
    Function(FunctionEnvironment),
    /// Stores module bindings, which include references to bindings on other environments.
    Module(ModuleEnvironment),
}

impl DeclarativeEnvironmentKind {
    /// Unwraps the inner function environment if possible. Returns `None` otherwise.
    pub(crate) const fn as_function(&self) -> Option<&FunctionEnvironment> {
        if let Self::Function(fun) = &self {
            Some(fun)
        } else {
            None
        }
    }

    /// Unwraps the inner global environment if possible. Returns `None` otherwise.
    pub(crate) const fn as_global(&self) -> Option<&GlobalEnvironment> {
        if let Self::Global(fun) = &self {
            Some(fun)
        } else {
            None
        }
    }

    /// Unwraps the inner module environment if possible. Returns `None` otherwise.
    pub(crate) const fn as_module(&self) -> Option<&ModuleEnvironment> {
        if let Self::Module(module) = &self {
            Some(module)
        } else {
            None
        }
    }

    /// Get the binding value from the environment by it's index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range or not initialized.
    #[track_caller]
    pub(crate) fn get(&self, index: u32) -> Option<JsValue> {
        match self {
            Self::Lexical(inner) => inner.get(index),
            Self::Global(inner) => inner.get(index),
            Self::Function(inner) => inner.get(index),
            Self::Module(inner) => inner.get(index),
        }
    }

    /// Sets the binding value from the environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range.
    #[track_caller]
    pub(crate) fn set(&self, index: u32, value: JsValue) {
        match self {
            Self::Lexical(inner) => inner.set(index, value),
            Self::Global(inner) => inner.set(index, value),
            Self::Function(inner) => inner.set(index, value),
            Self::Module(inner) => inner.set(index, value),
        }
    }

    /// `GetThisBinding`
    ///
    /// Returns the `this` binding of this environment.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-getthisbinding
    pub(crate) fn get_this_binding(&self) -> JsResult<Option<JsValue>> {
        match self {
            Self::Lexical(_) | Self::Global(_) => Ok(None),
            Self::Function(f) => f.get_this_binding(),
            Self::Module(_) => Ok(Some(JsValue::undefined())),
        }
    }

    /// `HasThisBinding`
    ///
    /// Returns if the environment has a `this` binding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-hasthisbinding
    pub(crate) fn has_this_binding(&self) -> bool {
        match self {
            Self::Lexical(_) => false,
            Self::Function(f) => f.has_this_binding(),
            Self::Global(_) | Self::Module(_) => true,
        }
    }

    /// Returns `true` if this environment is poisoned.
    pub(crate) fn poisoned(&self) -> bool {
        match self {
            Self::Lexical(lex) => lex.poisonable_environment().poisoned(),
            Self::Global(g) => g.poisonable_environment().poisoned(),
            Self::Function(f) => f.poisonable_environment().poisoned(),
            Self::Module(_) => false,
        }
    }

    /// Returns `true` if this environment is inside a `with` environment.
    pub(crate) fn with(&self) -> bool {
        match self {
            Self::Lexical(lex) => lex.poisonable_environment().with(),
            Self::Global(g) => g.poisonable_environment().with(),
            Self::Function(f) => f.poisonable_environment().with(),
            Self::Module(_) => false,
        }
    }

    /// Poisons this environment for future binding searches.
    pub(crate) fn poison(&self) {
        match self {
            Self::Lexical(lex) => lex.poisonable_environment().poison(),
            Self::Global(g) => g.poisonable_environment().poison(),
            Self::Function(f) => f.poisonable_environment().poison(),
            Self::Module(_) => {
                unreachable!("modules are always run in strict mode")
            }
        }
    }
}

#[derive(Debug, Trace, Finalize)]
pub(crate) struct PoisonableEnvironment {
    bindings: GcRefCell<Vec<Option<JsValue>>>,
    #[unsafe_ignore_trace]
    poisoned: Cell<bool>,
    with: bool,
}

impl PoisonableEnvironment {
    /// Creates a new `PoisonableEnvironment`.
    pub(crate) fn new(bindings_count: u32, poisoned: bool, with: bool) -> Self {
        Self {
            bindings: GcRefCell::new(vec![None; bindings_count as usize]),
            poisoned: Cell::new(poisoned),
            with,
        }
    }

    /// Gets the bindings of this poisonable environment.
    pub(crate) const fn bindings(&self) -> &GcRefCell<Vec<Option<JsValue>>> {
        &self.bindings
    }

    /// Gets the binding value from the environment by it's index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range.
    #[track_caller]
    fn get(&self, index: u32) -> Option<JsValue> {
        self.bindings.borrow()[index as usize].clone()
    }

    /// Sets the binding value from the environment by index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range.
    #[track_caller]
    pub(crate) fn set(&self, index: u32, value: JsValue) {
        self.bindings.borrow_mut()[index as usize] = Some(value);
    }

    /// Returns `true` if this environment is poisoned.
    fn poisoned(&self) -> bool {
        self.poisoned.get()
    }

    /// Returns `true` if this environment is inside a `with` environment.
    fn with(&self) -> bool {
        self.with
    }

    /// Poisons this environment for future binding searches.
    fn poison(&self) {
        self.poisoned.set(true);
    }
}

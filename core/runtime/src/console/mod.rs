//! Boa's implementation of JavaScript's `console` Web API object.
//!
//! The `console` object can be accessed from any global object.
//!
//! The specifics of how it works varies from browser to browser, but there is a de facto set of features that are typically provided.
//!
//! More information:
//!  - [MDN documentation][mdn]
//!  - [WHATWG `console` specification][spec]
//!
//! [spec]: https://console.spec.whatwg.org/
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Console

#[cfg(test)]
mod tests;

use boa_engine::property::Attribute;
use boa_engine::value::Convert;
use boa_engine::{
    js_str, js_string,
    native_function::NativeFunction,
    object::{JsObject, ObjectInitializer},
    value::{JsValue, Numeric},
    Context, JsArgs, JsData, JsError, JsResult, JsStr, JsString,
};
use boa_gc::{Finalize, Gc, GcRef, GcRefCell, GcRefMut, Trace};
use boa_interop::{ContextData, IntoJsFunctionCopied, JsRest, TryFromJsArgument};
use rustc_hash::FxHashMap;
use std::fmt::Write;
use std::ops::Deref;
use std::{cell::RefCell, collections::hash_map::Entry, rc::Rc, time::SystemTime};

/// A trait that can be used to forward console logs to an implementation.
pub trait Logger: Trace {
    /// Log a log message (`console.log`).
    ///
    /// # Errors
    /// Returning an error will throw an exception in JavaScript.
    fn log(&self, msg: String, state: &ConsoleState) -> JsResult<()>;

    /// Log an info message (`console.info`).
    ///
    /// # Errors
    /// Returning an error will throw an exception in JavaScript.
    fn info(&self, msg: String, state: &ConsoleState) -> JsResult<()>;

    /// Log a warning message (`console.warn`).
    ///
    /// # Errors
    /// Returning an error will throw an exception in JavaScript.
    fn warn(&self, msg: String, state: &ConsoleState) -> JsResult<()>;

    /// Log an error message (`console.error`).
    ///
    /// # Errors
    /// Returning an error will throw an exception in JavaScript.
    fn error(&self, msg: String, state: &ConsoleState) -> JsResult<()>;
}

/// The default implementation for logging from the console.
///
/// Implements the [`Logger`] trait and output errors to stderr and all
/// the others to stdout. Will add indentation based on the number of
/// groups.
#[derive(Trace, Finalize)]
struct DefaultLogger;

impl Logger for DefaultLogger {
    #[inline]
    fn log(&self, msg: String, state: &ConsoleState) -> JsResult<()> {
        let indent = 2 * state.groups.len();
        println!("{msg:>indent$}");
        Ok(())
    }

    #[inline]
    fn info(&self, msg: String, state: &ConsoleState) -> JsResult<()> {
        self.log(msg, state)
    }

    #[inline]
    fn warn(&self, msg: String, state: &ConsoleState) -> JsResult<()> {
        self.log(msg, state)
    }

    #[inline]
    fn error(&self, msg: String, state: &ConsoleState) -> JsResult<()> {
        let indent = 2 * state.groups.len();
        eprintln!("{msg:>indent$}");
        Ok(())
    }
}

/// This represents the `console` formatter.
fn formatter(data: &[JsValue], context: &mut Context) -> JsResult<String> {
    let mut s = String::new();
    match data {
        [] => {}
        [val] => {
            write!(s, "{}", val.to_string(context)?.display_escaped())
                .map_err(JsError::from_rust)?;
        }
        data => {
            let mut arg_index = 1;
            let target = data
                .get_or_undefined(0)
                .to_string(context)?
                .to_std_string_escaped();
            let mut it = target.chars();

            while let Some(c) = it.next() {
                if c == '%' {
                    let fmt = it.next().unwrap_or('%');
                    match fmt {
                        /* integer */
                        'd' | 'i' => {
                            match data.get_or_undefined(arg_index).to_numeric(context)? {
                                Numeric::Number(r) => {
                                    write!(s, "{}", r as i64).map_err(JsError::from_rust)?;
                                }
                                Numeric::BigInt(int) => {
                                    write!(s, "{int}").map_err(JsError::from_rust)?;
                                }
                            };
                            arg_index += 1;
                        }
                        /* float */
                        'f' => {
                            let arg = data.get_or_undefined(arg_index).to_number(context)?;
                            write!(s, "{arg:.6}").map_err(JsError::from_rust)?;
                            arg_index += 1;
                        }
                        /* object, FIXME: how to render this properly? */
                        'o' | 'O' => {
                            let arg = data.get_or_undefined(arg_index);
                            write!(s, "{}", arg.display_obj(true)).map_err(JsError::from_rust)?;
                            arg_index += 1;
                        }
                        /* string */
                        's' => {
                            write!(
                                s,
                                "{}",
                                data.get_or_undefined(arg_index)
                                    .to_string(context)?
                                    .display_escaped()
                            )
                            .map_err(JsError::from_rust)?;
                            arg_index += 1;
                        }
                        '%' => s.write_char('%').map_err(JsError::from_rust)?,
                        // %c is not implemented, but we still need to consume the argument.
                        'c' => arg_index += 1,
                        c => {
                            write!(s, "%{c}").map_err(JsError::from_rust)?;
                        }
                    }
                } else {
                    s.write_char(c).map_err(JsError::from_rust)?;
                };
            }

            /* unformatted data */
            for rest in data.iter().skip(arg_index) {
                write!(s, " {}", rest.to_string(context)?.display_escaped())
                    .map_err(JsError::from_rust)?;
            }
        }
    }

    Ok(s)
}

/// The internal data state passed to the functions through the context.
#[derive(Clone, Trace, Finalize, JsData)]
struct ConsoleData {
    state: Gc<GcRefCell<ConsoleState>>,
    logger: Gc<Box<dyn Logger>>,
}

impl ConsoleData {
    fn new<L: Logger + 'static + Sized>(logger: L) -> Self {
        Self {
            state: Default::default(),
            logger: Gc::new(Box::new(logger)),
        }
    }

    fn state(&self) -> GcRef<'_, ConsoleState> {
        self.state.borrow()
    }

    fn state_mut(&self) -> GcRefMut<'_, ConsoleState> {
        self.state.borrow_mut()
    }

    fn log(&self, msg: String) -> JsResult<()> {
        self.logger.log(msg, self.state.borrow().deref())
    }

    fn info(&self, msg: String) -> JsResult<()> {
        self.logger.info(msg, self.state.borrow().deref())
    }

    fn warn(&self, msg: String) -> JsResult<()> {
        self.logger.warn(msg, self.state.borrow().deref())
    }

    fn error(&self, msg: String) -> JsResult<()> {
        self.logger.error(msg, self.state.borrow().deref())
    }
}

/// This is the internal console object state.
#[derive(Default, Trace, Finalize, JsData)]
pub struct ConsoleState {
    /// The map of console counters, used in `console.count()`.
    pub count_map: FxHashMap<JsString, u32>,

    /// The map of console timers, used in `console.time`, `console.timeLog`
    /// and `console.timeEnd`.
    pub timer_map: FxHashMap<JsString, u128>,

    /// The current list of groups. Groups should be indented, but some logging
    /// libraries may want to use them in a different way.
    pub groups: Vec<String>,
}

pub struct FormattedJsArgs<'a> {
    args: &'a [JsValue],
}

impl<'a> TryFromJsArgument<'a> for FormattedJsArgs<'a> {
    fn try_from_js_argument(
        this: &'a JsValue,
        rest: &'a [JsValue],
        context: &mut Context,
    ) -> JsResult<(Self, &'a [JsValue])> {
    }
}

/// `console.assert(condition, ...data)`
///
/// Prints a JavaScript value to the standard error if first argument evaluates to `false` or there
/// were no arguments.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#assert
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/assert
fn assert(
    ContextData(console): ContextData<ConsoleData>,
    Convert(assertion): Convert<bool>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    if !assertion {
        let message = js_string!("Assertion failed");
        let mut args = args.to_vec();
        if args.is_empty() {
            args.push(JsValue::new(message));
        } else if !args[0].is_string() {
            args.insert(0, JsValue::new(message));
        } else {
            let value = JsString::from(args[0].display().to_string());
            let concat = js_string!(message.as_str(), js_str!(": "), &value);
            args[0] = JsValue::new(concat);
        }

        console.error(formatter(&args, context)?)?;
    }

    Ok(())
}

/// `console.clear()`
///
/// Removes all groups and clears console if possible.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#clear
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/clear
#[allow(clippy::unnecessary_wraps)]
fn clear(ContextData(console): ContextData<ConsoleData>) {
    console.state_mut().groups.clear();
}

/// `console.debug(...data)`
///
/// Prints a JavaScript values with "debug" logLevel.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#debug
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/debug
fn debug(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    console.log(formatter(args, context)?)
}

/// `console.error(...data)`
///
/// Prints a JavaScript values with "error" logLevel.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#error
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/error
fn error(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    console.error(formatter(args, context)?)
}

/// `console.info(...data)`
///
/// Prints a JavaScript values with "info" logLevel.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#info
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/info
fn info(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    console.info(formatter(args, context)?)
}

/// `console.log(...data)`
///
/// Prints a JavaScript values with "log" logLevel.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#log
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/log
fn log(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    let console = &*console.borrow();
    console.info(formatter(args, context)?)
}

/// `console.trace(...data)`
///
/// Prints a stack trace with "trace" logLevel, optionally labelled by data.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#trace
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/trace
fn trace(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    if !args.is_empty() {
        console.log(formatter(args, context)?)?;
    }

    let stack_trace_dump = context
        .stack_trace()
        .map(|frame| frame.code_block().name())
        .collect::<Vec<_>>()
        .into_iter()
        .map(JsString::to_std_string_escaped)
        .collect::<Vec<_>>()
        .join("\n");
    console.log(stack_trace_dump)
}

/// `console.warn(...data)`
///
/// Prints a JavaScript values with "warn" logLevel.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#warn
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/warn
fn warn(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    console.warn(formatter(args, context)?)
}

/// `console.count(label)`
///
/// Prints number of times the function was called with that particular label.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#count
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/count
fn count(
    ContextData(console): ContextData<ConsoleData>,
    label: Option<Convert<JsString>>,
    context: &mut Context,
) -> JsResult<()> {
    let label = label.map_or(js_string!("default"), |c| c.0);

    let msg = format!("count {}:", label.display_escaped());
    let c = console.state_mut().count_map.entry(label).or_insert(0);
    *c += 1;

    console.info(format!("{msg} {c}"))
}

/// `console.countReset(label)`
///
/// Resets the counter for label.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#countreset
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/countReset
fn count_reset(
    ContextData(console): ContextData<ConsoleData>,
    label: Option<Convert<JsString>>,
    context: &mut Context,
) -> JsResult<()> {
    let label = label.map_or(js_string!("default"), |c| c.0);
    console.state_mut().count_map.remove(&label);

    console.warn(
        format!("countReset {}", label.to_std_string_escaped()),
        console,
    )?;

    Ok(())
}

/// Returns current system time in ms.
fn system_time_in_ms() -> u128 {
    let now = SystemTime::now();
    now.duration_since(SystemTime::UNIX_EPOCH)
        .expect("negative duration")
        .as_millis()
}

/// `console.time(label)`
///
/// Starts the timer for given label.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#time
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/time
fn time(
    ContextData(console): ContextData<ConsoleData>,
    label: Option<Convert<JsString>>,
    context: &mut Context,
) -> JsResult<()> {
    let label = label.map_or(js_string!("default"), |c| c.0);

    match console.state_mut().timer_map.entry(label) {
        Entry::Occupied(e) => {
            console.warn(format!(
                "Timer '{}' already exist",
                e.key().display_escaped()
            ))?;
        }
        Entry::Vacant(e) => {
            let time = system_time_in_ms();
            e.insert(time);
        }
    }

    Ok(())
}

/// `console.timeLog(label, ...data)`
///
/// Prints elapsed time for timer with given label.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#timelog
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/timeLog
fn time_log(
    ContextData(console): ContextData<ConsoleData>,
    label: Option<Convert<JsString>>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    let label = label.map_or(js_string!("default"), |c| c.0);

    console.state_mut().timer_map.get(&label).map_or_else(
        || console.warn(format!("Timer '{}' doesn't exist", label.display_escaped())),
        |t| {
            let time = system_time_in_ms();
            let mut concat = format!("{}: {} ms", label.display_escaped(), time - t);
            for msg in args {
                write!(concat, " {}", msg.display()).map_err(JsError::from_rust)?;
            }
            console.log(concat)
        },
    )?;

    Ok(())
}

/// `console.timeEnd(label)`
///
/// Removes the timer with given label.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#timeend
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/timeEnd
fn time_end(
    ContextData(console): ContextData<ConsoleData>,
    label: Option<Convert<JsString>>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    let label = label.map_or(js_string!("default"), |c| c.0);

    console.state_mut().timer_map.remove(&label).map_or_else(
        || console.warn(format!("Timer '{}' doesn't exist", label.display_escaped())),
        |t| {
            let time = Self::system_time_in_ms();
            console.info(format!(
                "{}: {} ms - timer removed",
                label.display_escaped(),
                time - t
            ))
        },
    )?;

    Ok(())
}

/// `console.group(...data)`
///
/// Adds new group with name from formatted data to stack.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#group
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/group
fn group(
    ContextData(console): ContextData<ConsoleData>,
    JsRest(args): JsRest<'_>,
    context: &mut Context,
) -> JsResult<()> {
    let group_label = formatter(args, context)?;

    logger.info(format!("group: {group_label}"), console)?;
    console.groups.push(group_label);

    Ok(JsValue::undefined())
}

/// `console.groupCollapsed(...data)`
///
/// Adds new group collapsed with name from formatted data to stack.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#groupcollapsed
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/groupcollapsed_static
fn group_collapsed(
    _: &JsValue,
    args: &[JsValue],
    console: &mut Self,
    logger: &impl Logger,
    context: &mut Context,
) -> JsResult<JsValue> {
    Console::group(&JsValue::Undefined, args, console, logger, context)
}

/// `console.groupEnd(label)`
///
/// Removes the last group from the stack.
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#groupend
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/groupEnd
#[allow(clippy::unnecessary_wraps)]
fn group_end(
    _: &JsValue,
    _: &[JsValue],
    console: &mut Self,
    _: &impl Logger,
    _: &mut Context,
) -> JsResult<JsValue> {
    console.groups.pop();

    Ok(JsValue::undefined())
}

/// `console.dir(item, options)`
///
/// Prints info about item
///
/// More information:
///  - [MDN documentation][mdn]
///  - [WHATWG `console` specification][spec]
///
/// [spec]: https://console.spec.whatwg.org/#dir
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/console/dir
#[allow(clippy::unnecessary_wraps)]
fn dir(
    _: &JsValue,
    args: &[JsValue],
    console: &Self,
    logger: &impl Logger,
    _: &mut Context,
) -> JsResult<JsValue> {
    logger.info(args.get_or_undefined(0).display_obj(true), console)?;
    Ok(JsValue::undefined())
}

/// The console object.
#[derive(Debug, Default, Trace, Finalize, JsData)]
pub struct Console;

impl Console {
    /// Name of the built-in `console` property.
    pub const NAME: JsStr<'static> = js_str!("console");

    /// Initializes the `console` with a special logger.
    #[allow(clippy::too_many_lines)]
    pub fn init_with_logger<L>(context: &mut Context, logger: L) -> JsObject
    where
        L: Logger + 'static,
    {
        fn console_method<L: Logger + 'static>(
            f: fn(&JsValue, &[JsValue], &Console, &L, &mut Context) -> JsResult<JsValue>,
            state: Rc<RefCell<Console>>,
            logger: Rc<L>,
        ) -> NativeFunction {
            // SAFETY: `Console` doesn't contain types that need tracing.
            unsafe {
                NativeFunction::from_closure(move |this, args, context| {
                    f(this, args, &state.borrow(), &logger, context)
                })
            }
        }
        fn console_method_mut<L: Logger + 'static>(
            f: fn(&JsValue, &[JsValue], &mut Console, &L, &mut Context) -> JsResult<JsValue>,
            state: Rc<RefCell<Console>>,
            logger: Rc<L>,
        ) -> NativeFunction {
            // SAFETY: `Console` doesn't contain types that need tracing.
            unsafe {
                NativeFunction::from_closure(move |this, args, context| {
                    f(this, args, &mut state.borrow_mut(), &logger, context)
                })
            }
        }
        // let _timer = Profiler::global().start_event(std::any::type_name::<Self>(), "init");

        let state = Gc::new(GcRefCell::new(ConsoleState::new(logger)));
        context.insert_data(state.clone());

        ObjectInitializer::with_native_data(Self, context)
            .function(
                assert.into_js_function_copied(context),
                js_string!("assert"),
                0,
            )
            .function(
                clear.into_js_function_copied(context),
                js_string!("clear"),
                0,
            )
            .function(
                debug.into_js_function_copied(context),
                js_string!("debug"),
                0,
            )
            // .function(
            //     console_method(Self::error, state.clone(), logger.clone()),
            //     js_string!("error"),
            //     0,
            // )
            // .function(
            //     console_method(Self::info, state.clone(), logger.clone()),
            //     js_string!("info"),
            //     0,
            // )
            // .function(
            //     console_method(Self::log, state.clone(), logger.clone()),
            //     js_string!("log"),
            //     0,
            // )
            // .function(
            //     console_method(Self::trace, state.clone(), logger.clone()),
            //     js_string!("trace"),
            //     0,
            // )
            // .function(
            //     console_method(Self::warn, state.clone(), logger.clone()),
            //     js_string!("warn"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::count, state.clone(), logger.clone()),
            //     js_string!("count"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::count_reset, state.clone(), logger.clone()),
            //     js_string!("countReset"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::group, state.clone(), logger.clone()),
            //     js_string!("group"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::group_collapsed, state.clone(), logger.clone()),
            //     js_string!("groupCollapsed"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::group_end, state.clone(), logger.clone()),
            //     js_string!("groupEnd"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::time, state.clone(), logger.clone()),
            //     js_string!("time"),
            //     0,
            // )
            // .function(
            //     console_method(Self::time_log, state.clone(), logger.clone()),
            //     js_string!("timeLog"),
            //     0,
            // )
            // .function(
            //     console_method_mut(Self::time_end, state.clone(), logger.clone()),
            //     js_string!("timeEnd"),
            //     0,
            // )
            // .function(
            //     console_method(Self::dir, state.clone(), logger.clone()),
            //     js_string!("dir"),
            //     0,
            // )
            // .function(
            //     console_method(Self::dir, state, logger.clone()),
            //     js_string!("dirxml"),
            //     0,
            // )
            .build()
    }

    /// Initializes the `console` built-in object.
    pub fn init(context: &mut Context) -> JsObject {
        Self::init_with_logger(context, DefaultLogger)
    }

    /// Register the console as a global `console` object in the context.
    ///
    /// # Errors
    /// Any errors from registering the global property will be reported.
    pub fn register(context: &mut Context) -> JsResult<()> {
        let console = Console::init(context);
        context.register_global_property(js_string!(Console::NAME), console, Attribute::all())
    }
}

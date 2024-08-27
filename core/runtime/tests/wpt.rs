use boa_engine::property::Attribute;
use boa_engine::{js_str, Context, JsResult, Source};
use boa_engine::{Finalize, Trace};
use boa_gc::{Gc, GcRefCell};
use boa_runtime::{Console, Logger};
use rstest::*;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A unique index of all logs.
static UNIQUE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Trace, Finalize)]
struct RecordingLogEvent {
    pub index: usize,
    pub indent: usize,
    pub msg: String,
}

impl RecordingLogEvent {
    pub fn new(msg: String, state: &Console) -> Self {
        Self {
            index: UNIQUE.fetch_add(1, Ordering::SeqCst),
            indent: state.groups.len(),
            msg,
        }
    }
}

#[derive(Trace, Finalize)]
struct RecordingLogger {
    pub log: Gc<GcRefCell<Vec<RecordingLogEvent>>>,
    pub error: Gc<GcRefCell<Vec<RecordingLogEvent>>>,
}

impl Logger for RecordingLogger {
    fn log(&self, msg: String, state: &Console) -> JsResult<()> {
        self.log
            .borrow_mut()
            .push(RecordingLogEvent::new(msg, state));
        Ok(())
    }

    fn info(&self, msg: String, state: &Console) -> JsResult<()> {
        self.log
            .borrow_mut()
            .push(RecordingLogEvent::new(msg, state));
        Ok(())
    }

    fn warn(&self, msg: String, state: &Console) -> JsResult<()> {
        self.log
            .borrow_mut()
            .push(RecordingLogEvent::new(msg, state));
        Ok(())
    }

    fn error(&self, msg: String, state: &Console) -> JsResult<()> {
        self.error
            .borrow_mut()
            .push(RecordingLogEvent::new(msg, state));
        Ok(())
    }
}

// Test the console with the WPT test suite.
#[rstest]
fn console(#[files("../../wpt/console/*.js")] path: PathBuf) {
    let context = &mut Context::default();

    let log = Gc::new(GcRefCell::new(Vec::new()));
    let error = Gc::new(GcRefCell::new(Vec::new()));
    Console::init_with_logger(
        context,
        RecordingLogger {
            log: log.clone(),
            error: error.clone(),
        },
    );

    // Define self as the globalThis.
    let global_this = context.global_object();
    context
        .register_global_property(js_str!("self"), global_this, Attribute::all())
        .unwrap();

    let harness_path = PathBuf::from("../../wpt/resources/testharness.js");
    let harness = Source::from_reader(File::open(&harness_path).unwrap(), Some(&harness_path));
    context.eval(harness).unwrap();

    let source = Source::from_reader(File::open(&path).unwrap(), Some(&path));
    context.eval(source).unwrap();
}

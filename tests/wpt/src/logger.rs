use boa_engine::{Finalize, JsResult, Trace};
use boa_gc::{Gc, GcRefCell};
use boa_runtime::{Console, Logger};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A unique index of all logs.
static UNIQUE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Trace, Finalize)]
pub struct RecordingLogEvent {
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
pub struct RecordingLogger {
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

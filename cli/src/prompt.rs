use crate::logger::SharedExternalPrinterLogger;
use crate::{evaluate_expr, helper, uncaught_job_error};
use boa_engine::Context;
use color_eyre::Help;
use color_eyre::eyre::{WrapErr, eyre};
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::fs::OpenOptions;
use std::sync::mpsc::{Sender, TryRecvError};
use std::time::Duration;
use std::{io, thread};

/// A command that is sent from the readline thread to the main thread.
enum Command {
    /// Evaluate a JavaScript expression in the context.
    EvalExpression(String),
}

/// CLI configuration for Boa.
static CLI_HISTORY: &str = ".boa_history";

pub(crate) fn run(
    printer: &SharedExternalPrinterLogger,
    args: &super::Opt,
    context: &mut Context,
) -> color_eyre::Result<()> {
    let (sender, receiver) = std::sync::mpsc::channel::<Command>();
    let handle = start_readline_thread(sender, printer.clone(), args.vi_mode);

    loop {
        match receiver.try_recv() {
            Ok(Command::EvalExpression(line)) => {
                evaluate_expr(&line, args, context, printer)?;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        if let Err(err) = context.run_jobs() {
            printer.print(uncaught_job_error(&err));
        }
        thread::sleep(Duration::from_millis(10));
    }

    handle.join().expect("failed to join thread");
    Ok(())
}

fn readline_thread_main(
    sender: &Sender<Command>,
    printer_out: &SharedExternalPrinterLogger,
    vi_mode: bool,
) -> color_eyre::Result<()> {
    let config = Config::builder()
        .keyseq_timeout(Some(1))
        .edit_mode(if vi_mode {
            EditMode::Vi
        } else {
            EditMode::Emacs
        })
        .completion_type(CompletionType::List)
        .build();

    let mut editor =
        Editor::with_config(config).wrap_err("failed to set the editor configuration")?;
    if let Ok(printer) = editor.create_external_printer() {
        printer_out.set(printer);
    }

    // Check if the history file exists. If it doesn't, create it.
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(CLI_HISTORY)?;
    editor
        .load_history(CLI_HISTORY)
        .wrap_err("failed to read history file `.boa_history`")?;
    let readline = ">> ";
    editor.set_helper(Some(helper::RLHelper::new(readline)));

    loop {
        match editor.readline(readline) {
            Ok(line) if line == ".exit" => break,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,

            Ok(line) => {
                let line = line.trim_end();
                editor.add_history_entry(line).map_err(io::Error::other)?;
                sender.send(Command::EvalExpression(line.to_string()))?;
                thread::sleep(Duration::from_millis(10));
            }

            Err(err) => {
                let final_error = eyre!("could not read the next line of the input");
                let final_error = if let Err(e) = editor.save_history(CLI_HISTORY) {
                    final_error.error(e)
                } else {
                    final_error
                };
                return Err(final_error.error(err));
            }
        }
    }

    editor.save_history(CLI_HISTORY)?;

    Ok(())
}

/// Create the readline thread which sends lines from stdin back to the main thread.
fn start_readline_thread(
    sender: Sender<Command>,
    printer_out: SharedExternalPrinterLogger,
    vi_mode: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(
        move || match readline_thread_main(&sender, &printer_out, vi_mode) {
            Ok(()) => {}
            Err(e) => eprintln!("readline thread failed: {e}"),
        },
    )
}

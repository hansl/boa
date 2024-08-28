//! [WPT][https://web-platform-tests.org/] test runner
//!
//! This crate will run supported test suites from WPT and report compliance of the
//! `boa` engine.
//!
//! We do not plan to support the entire WPT suite. Things like CSS, HTML files,
//! and some APIs that are not relevant to the `boa` engine will not be supported,
//! ever. These would be more the domain of a browser rendering engine.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![allow(
    clippy::too_many_lines,
    clippy::redundant_pub_crate,
    clippy::cast_precision_loss,
    clippy::print_stderr,
    clippy::print_stdout
)]

use boa_engine::property::Attribute;
use boa_engine::{js_str, Context, Source};
use boa_gc::{Gc, GcRefCell};
use clap::{Parser, ValueHint};
use clap_verbosity_flag::Level as ClapVerbosityLevel;
use clap_verbosity_flag::Verbosity;
use color_eyre::{
    eyre::{bail, eyre, WrapErr},
    Result,
};
use fast_glob::glob_match;
use quick_junit::{Report, TestSuite};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fs::File;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};
use tracing::{debug, info, warn, Level};
use tracing_subscriber::fmt::Subscriber;
use walkdir::WalkDir;

mod logger;

static START: OnceLock<Instant> = OnceLock::new();

/// Structure that contains the configuration of the tester.
#[derive(Debug, Deserialize)]
struct Config {
    /// The `WPT` repository commit.
    #[serde(default)]
    pub(crate) commit: String,

    /// The list of tests to run (by default). This is a GLOB and can contain wildcards.
    #[serde(default)]
    pub(crate) included: FxHashSet<String>,

    /// The list of tests to skip. This is a GLOB and can contain wildcards.
    #[serde(default)]
    pub(crate) excluded: FxHashSet<String>,
}

impl Config {
    pub(crate) fn commit(&self) -> &str {
        &self.commit
    }
}

/// Boa WPT tester
#[derive(Debug, Parser)]
#[command(author, version, about, name = "Boa wpt tester")]
struct Cli {
    #[command(flatten)]
    pub verbose: Verbosity<clap_verbosity_flag::InfoLevel>,

    /// Path to a TOML file containing tester config.
    #[arg(short, long, default_value = "wpt_config.toml", value_hint = ValueHint::FilePath)]
    config: PathBuf,

    #[command(subcommand)]
    pub cmd: SubCommand,
}

/// Subcommands for the tester.
#[derive(Debug, Parser)]
enum SubCommand {
    /// Run the test suite.
    Run {
        /// Path to the WPT suite.
        #[arg(long, value_hint = ValueHint::DirPath)]
        wpt_path: PathBuf,

        /// Which specific test or test suite to run. Should be a path relative to the Test262
        /// directory: e.g. "console/console-label-conversion". This supports glob wildcards,
        /// and can be specified multiple times.
        #[arg(value_hint = ValueHint::AnyPath)]
        suite: Option<Vec<String>>,

        /// Exclude paths from the suites. This supports glob wildcards, and can be specified
        /// multiple times.
        #[arg(long, value_hint = ValueHint::AnyPath)]
        exclude: Option<Vec<String>>,

        /// Optional output folder for the full results information.
        #[arg(short, long, value_hint = ValueHint::DirPath)]
        output: Option<PathBuf>,
    },
}

/// Program entry point.
fn main() -> Result<()> {
    color_eyre::install()?;

    // Safety: This is needed because we run tests in multiple threads.
    // It is safe because tests do not modify the environment.
    unsafe {
        time::util::local_offset::set_soundness(time::util::local_offset::Soundness::Unsound);
    }

    // initializes the monotonic clock.
    START
        .set(Instant::now())
        .map_err(|_| eyre!("could not initialize the monotonic clock"))?;

    let opts = Cli::parse();

    // Initialize tracing with some sensible defaults for the CLI.
    // Tracing will done to STDERR. Anything that needs output to STDOUT should use `println!`,
    // e.g. test results and reports.
    let subscriber = Subscriber::builder();
    let subscriber = match opts.verbose.log_level() {
        Some(ClapVerbosityLevel::Error) => subscriber.with_max_level(Level::ERROR),
        Some(ClapVerbosityLevel::Warn) => subscriber.with_max_level(Level::WARN),
        Some(ClapVerbosityLevel::Info) => subscriber.with_max_level(Level::INFO),
        Some(ClapVerbosityLevel::Debug) => subscriber.with_max_level(Level::DEBUG),
        None | Some(ClapVerbosityLevel::Trace) => subscriber.with_max_level(Level::TRACE),
    };
    subscriber
        .with_ansi(true)
        .with_target(false)
        .compact()
        .with_writer(std::io::stderr)
        .init();

    let config: Config = {
        let input = std::fs::read_to_string(&opts.config)
            .wrap_err_with(|| eyre!("could not read config file `{}`", opts.config.display()))?;
        toml::from_str(&input)
            .wrap_err_with(|| eyre!("invalid config file `{}`", opts.config.display()))?
    };

    match opts.cmd {
        SubCommand::Run {
            wpt_path,
            suite,
            exclude,
            output,
        } => run_tests(&config, wpt_path, suite, exclude, output),
    }
}

/// Run the tests.
fn run_tests(
    config: &Config,
    wpt_path: PathBuf,
    suite: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    output: Option<PathBuf>,
) -> Result<()> {
    let harness_path = wpt_path.join("resources/testharness.js");

    let all_tests = get_all_tests(config, &wpt_path, suite.as_deref(), exclude.as_deref())?;

    info!(count = all_tests.len(), "Running tests...");

    let mut report = Report::new("boa-wpt");

    for (suite_name, tests) in all_tests {
        info!(?suite_name, count = tests.len(), "Running test...");
        let test_suite = TestSuite::new(&suite_name);

        for (test_name, full_path) in tests {
            let test_path = full_path.strip_prefix(&wpt_path)?;

            let log = Gc::new(GcRefCell::new(Vec::new()));
            let error = Gc::new(GcRefCell::new(Vec::new()));

            let context = &mut Context::default();
            boa_runtime::Console::init_with_logger(
                context,
                logger::RecordingLogger {
                    log: log.clone(),
                    error: error.clone(),
                },
            );

            // Define self as the globalThis.
            let global_this = context.global_object();
            context
                .register_global_property(js_str!("self"), global_this, Attribute::all())
                .unwrap();

            // Define the WPT test harness.
            let harness_source =
                Source::from_reader(File::open(&harness_path).unwrap(), Some(&harness_path));
            context.eval(harness_source).unwrap();

            // Execute the test.
            let source = Source::from_reader(File::open(&full_path).unwrap(), Some(&full_path));
            info!("Result: {:?}", context.eval(source).unwrap().display());

            info!("{:?}", log.borrow());
            warn!("{:?}", error.borrow());
        }

        report.add_test_suite(test_suite);
    }

    // Write the report to the output or the STDOUT.
    if let Some(output) = output {
        let output = output.join("report.xml");
        std::fs::write(&output, report.to_string().unwrap())?;
        info!(path = ?output, "Report written to file");
    } else {
        println!("{}", report.to_string().unwrap());
    }

    Ok(())
}

/// List all tests from a directory with an optional exclusion list.
fn list_all_tests(
    root: &Path,
    pattern: &String,
    excludes: &FxHashSet<String>,
) -> Result<FxHashSet<PathBuf>> {
    let pattern = if pattern.ends_with('/') {
        pattern.clone() + "**"
    } else if pattern.contains('/') {
        pattern.clone()
    } else {
        pattern.clone() + "/**"
    };

    // let pattern = root.join(pattern);
    Ok(WalkDir::new(root)
        .into_iter()
        // Filter out errors.
        .filter_map(Result::ok)
        // Filter out non-files.
        .filter(|entry| entry.file_type().is_file())
        // Filter out non-JS extension.
        .filter(|entry| entry.path().extension() == Some(OsStr::new("js")))
        // Filter out non pattern-matching files.
        .filter(|entry| {
            let p = entry.path().strip_prefix(root).unwrap();
            glob_match(&pattern, p.to_string_lossy().as_ref())
        })
        // Filter out excluded patterns.
        .filter(|entry| {
            let p = entry.path().strip_prefix(root).unwrap().to_string_lossy();
            !excludes.iter().any(|g| glob_match(g, p.as_ref()))
        })
        .map(|entry| entry.path().to_path_buf())
        .collect())
}

/// Returns a sanitized version (with non-ascii and non-alphanumeric characters replaced etc)
/// of a suite/test name.
fn sanitize_name(name: &str) -> String {
    name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        .replace('/', "-")
}

/// Returns the name of the test suite from the path.
fn suite_and_test_name_from_path(root: &Path, path: &Path) -> Result<(String, String)> {
    let rel_path = path.strip_prefix(root)?;
    let mut parts = rel_path.components().collect::<Vec<_>>();
    let suite = parts.remove(0);
    let mut test = parts
        .into_iter()
        .map(|p| p.as_os_str())
        .collect::<PathBuf>();
    test.set_extension("");

    Ok((
        sanitize_name(suite.as_os_str().to_string_lossy().as_ref()),
        sanitize_name(test.to_string_lossy().as_ref()),
    ))
}

/// Get all the tests to run from a WPT root, a config and an optional vector of suite.
fn get_all_tests(
    config: &Config,
    wpt_path: &Path,
    suites: Option<&[String]>,
    exclude: Option<&[String]>,
) -> Result<FxHashMap<String, FxHashMap<String, PathBuf>>> {
    let mut all_tests: FxHashMap<String, FxHashMap<String, PathBuf>> = FxHashMap::default();

    let exclude = exclude
        .unwrap_or_default()
        .iter()
        .cloned()
        .collect::<FxHashSet<_>>();

    if let Some(suite_slice) = suites {
        for suite in suite_slice {
            let all_files = list_all_tests(wpt_path, suite, &exclude)?;
            if all_files.is_empty() {
                bail!("no test files found in suite: `{suite}`");
            }
            eprintln!("all_files: {:?}", all_files);
            eprintln!("exclude: {:?}", exclude);
            for file in all_files {
                let (suite_name, test_name) = suite_and_test_name_from_path(wpt_path, &file)?;
                all_tests
                    .entry(suite_name)
                    .or_default()
                    .insert(test_name, file);
            }
        }
    } else {
        let exclusions = config.excluded.iter().cloned().collect::<FxHashSet<_>>();
        let exclusions = exclude
            .union(&exclusions)
            .cloned()
            .collect::<FxHashSet<_>>();

        for suite in &config.included {
            let all_files = list_all_tests(wpt_path, &suite, &exclusions)?;
            if all_files.is_empty() {
                bail!("no test files found in suite: `{suite}`");
            }

            for file in all_files {
                let (suite_name, test_name) = suite_and_test_name_from_path(wpt_path, &file)?;
                all_tests
                    .entry(suite_name)
                    .or_default()
                    .insert(test_name, file);
            }
        }
    }

    Ok(all_tests)
}

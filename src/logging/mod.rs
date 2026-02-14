pub mod config;
pub mod context;
pub mod layers;

pub use context::{detect_context, ExecutionContext};
pub use layers::console::ConsoleOutput;

use crate::logging::config::LoggingConfig;
use crate::logging::layers::{console, file, opentelemetry};
use crate::{cli::Command, core::batch_config::find_workspace_root, Result};
use anyhow::{anyhow, Context};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::Registry;

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Guards that keep logging sinks active for the duration of the command.
pub struct LoggingGuard {
    _file_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    otel_guard: Option<layers::opentelemetry::OpenTelemetryHandle>,
    console_output: ConsoleOutput,
    log_file_path: PathBuf,
}

impl LoggingGuard {
    /// Returns the console output configuration used during initialization.
    pub fn console_output(&self) -> ConsoleOutput {
        self.console_output
    }

    /// Returns the log file path backed by the file sink.
    pub fn log_file_path(&self) -> &Path {
        &self.log_file_path
    }
}

impl Drop for LoggingGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.otel_guard.take() {
            handle.shutdown();
        }
    }
}

/// Initialize the logging framework for the provided CLI command.
///
/// This function configures filters, file sinks, console sinks, and optional OpenTelemetry
/// export based on deterministic configuration precedence. It errors when invoked more than once
/// per process invocation unless tests explicitly reset the guard.
pub fn init(command: &Command) -> Result<LoggingGuard> {
    if LOGGER_INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(anyhow!("logging already initialized"));
    }

    let context = detect_context(command);
    let workspace_root = resolve_workspace_path(command);
    let config = LoggingConfig::load(workspace_root.as_deref())?;

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.default_level))
        .context("failed to configure tracing level")?;
    let log_file_path = layers::file::log_file_path(&config, workspace_root.as_deref())?;
    type BaseRegistry = Registry;
    type FileSubscriber = file::FileLayerStack<BaseRegistry>;
    type ConsoleSubscriber = console::ConsoleLayerStack<FileSubscriber>;

    let (file_layer, file_guard) =
        file::file_layer::<BaseRegistry>(&log_file_path, config.enable_file)?;

    let subscriber = tracing_subscriber::registry();
    let subscriber = subscriber.with(file_layer);

    let console_output = console::select_console_output(context, config.console_output);
    let console_layer = console::console_layer::<FileSubscriber>(console_output);
    let subscriber = subscriber.with(console_layer);

    let (otel_layer, otel_handle) = if config.opentelemetry.enabled {
        match opentelemetry::init::<ConsoleSubscriber>(&config.opentelemetry) {
            Ok((layer, handle)) => (opentelemetry::OptionalLayer::enabled(layer), Some(handle)),
            Err(err) => {
                tracing::warn!("OpenTelemetry disabled: {}", err);
                (opentelemetry::OptionalLayer::disabled(), None)
            }
        }
    } else {
        (opentelemetry::OptionalLayer::disabled(), None)
    };

    let subscriber = subscriber.with(otel_layer);
    let subscriber = subscriber.with(env_filter);
    subscriber.init();

    Ok(LoggingGuard {
        _file_guard: file_guard,
        otel_guard: otel_handle,
        console_output,
        log_file_path,
    })
}

fn resolve_workspace_path(command: &Command) -> Option<PathBuf> {
    match command {
        Command::Run(args) => Some(args.path.clone()),
        Command::Init(args) => args.path.clone().or_else(|| env::current_dir().ok()),
        Command::Batch(args) => args.workspace.clone().or_else(|| {
            env::current_dir()
                .ok()
                .and_then(|cwd| find_workspace_root(&cwd).ok())
        }),
        Command::Step(args) => Some(args.path.clone()),
        Command::Status(args) => Some(args.path.clone()),
        Command::Report(args) => Some(args.path.clone()),
        Command::Error(_) => env::current_dir().ok(),
        Command::Monitor(_) => env::current_dir()
            .ok()
            .and_then(|cwd| find_workspace_root(&cwd).ok()),
    }
}

#[cfg(test)]
/// Reset the initialization guard so tests can reconfigure logging multiple times.
pub fn reset_for_tests() {
    LOGGER_INITIALIZED.store(false, Ordering::SeqCst);
}

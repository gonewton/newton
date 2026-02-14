use crate::logging::config::LoggingConfig;
use crate::Result;
use anyhow::{anyhow, Context};
use dirs::home_dir;
use std::fs::{create_dir_all, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use tracing::Subscriber;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::{self as tracing_fmt, format, writer::BoxMakeWriter};
use tracing_subscriber::registry::LookupSpan;

/// Layer type produced by the file sink builder.
pub type FileFmtLayer<S> =
    tracing_fmt::Layer<S, format::DefaultFields, format::Format<format::Full>, BoxMakeWriter>;

/// Layer stack that already wraps the provided subscriber.
pub type FileLayerStack<S> = tracing_subscriber::layer::Layered<FileFmtLayer<S>, S>;

/// Determine the file layout used by the logging file sink.
pub fn log_file_path(config: &LoggingConfig, workspace_root: Option<&Path>) -> Result<PathBuf> {
    let directory = resolve_log_dir(config, workspace_root)?;
    Ok(directory.join("newton.log"))
}

/// Build a tracing layer that writes to the provided file path via a non-blocking writer.
pub fn file_layer<S>(
    log_file: &Path,
    enabled: bool,
) -> Result<(FileFmtLayer<S>, Option<WorkerGuard>)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    if enabled {
        ensure_log_dir(log_file)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .with_context(|| format!("failed to open log file {}", log_file.display()))?;

        let (non_blocking, guard) = tracing_appender::non_blocking(file);
        let writer = BoxMakeWriter::new(move || non_blocking.clone());
        let layer = make_layer(writer);
        Ok((layer, Some(guard)))
    } else {
        let writer = BoxMakeWriter::new(io::sink);
        let layer = make_layer(writer);
        Ok((layer, None))
    }
}

fn make_layer<S>(writer: BoxMakeWriter) -> FileFmtLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    tracing_fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
}

fn ensure_log_dir(log_file: &Path) -> Result<()> {
    let directory = log_file.parent().ok_or_else(|| {
        anyhow!(
            "log file path {} has no parent directory",
            log_file.display()
        )
    })?;
    create_dir_all(directory)
        .with_context(|| format!("failed to create log directory {}", directory.display()))?;
    Ok(())
}

fn resolve_log_dir(config: &LoggingConfig, workspace_root: Option<&Path>) -> Result<PathBuf> {
    let base_dir = if let Some(custom) = &config.log_dir {
        if custom.is_absolute() {
            custom.clone()
        } else if let Some(workspace) = workspace_root {
            workspace.join(custom)
        } else {
            home_base()?.join(custom)
        }
    } else if let Some(workspace) = workspace_root {
        workspace.join(".newton").join("logs")
    } else {
        home_base()?.join(".newton").join("logs")
    };

    let normalized = canonicalize_or_clone(&base_dir);
    ensure_within_anchor(&normalized, workspace_root, &config.log_dir)?;
    Ok(normalized)
}

fn home_base() -> Result<PathBuf> {
    home_dir().ok_or_else(|| anyhow!("$HOME directory unavailable"))
}

fn canonicalize_or_clone(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn ensure_within_anchor(
    candidate: &Path,
    workspace_root: Option<&Path>,
    override_dir: &Option<PathBuf>,
) -> Result<()> {
    if let Some(custom) = override_dir {
        if custom.is_absolute() {
            return Ok(());
        }
        if let Some(workspace) = workspace_root {
            let anchor = canonicalize_or_clone(workspace);
            if !candidate.starts_with(&anchor) {
                return Err(anyhow!(
                    "logging.log_dir resolves outside workspace {}",
                    anchor.display()
                ));
            }
        } else {
            let anchor = canonicalize_or_clone(&home_base()?);
            if !candidate.starts_with(&anchor) {
                return Err(anyhow!(
                    "logging.log_dir resolves outside home {}",
                    anchor.display()
                ));
            }
        }
    }
    Ok(())
}

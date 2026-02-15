use crate::logging::layers::BoxLayer;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::path::Path;
use tracing::Subscriber;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{fmt, registry::LookupSpan};

/// Creates a non-blocking file layer plus the guard that keeps the worker alive.
pub fn build_file_layer<S>(path: &Path) -> Result<(BoxLayer<S>, WorkerGuard)>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open log file {}", path.display()))?;

    let (non_blocking_writer, guard) = NonBlocking::new(file);
    let layer = fmt::layer()
        .with_writer(non_blocking_writer)
        .with_ansi(false)
        .with_thread_names(true)
        .with_thread_ids(true)
        .with_timer(fmt::time::UtcTime::rfc_3339());

    Ok((Box::new(layer), guard))
}

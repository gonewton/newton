use crate::logging::config::ConsoleOutput;
use crate::logging::layers::BoxLayer;
use std::io;
use tracing::Subscriber;
use tracing_subscriber::{fmt, registry::LookupSpan};

/// Builds a console logging layer or `None` when the chosen target is disabled.
pub fn build_console_layer<S>(output: ConsoleOutput) -> Option<BoxLayer<S>>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    match output {
        ConsoleOutput::None => None,
        ConsoleOutput::Stdout => Some(build_with_writer(io::stdout)),
        ConsoleOutput::Stderr => Some(build_with_writer(io::stderr)),
    }
}

fn build_with_writer<S, W>(writer: W) -> BoxLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
    W: for<'a> fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    Box::new(
        fmt::layer()
            .with_writer(writer)
            .with_ansi(true)
            .with_thread_names(true)
            .with_thread_ids(true),
    )
}

use crate::logging::context::ExecutionContext;
use serde::Deserialize;
use std::fmt;
use std::io;
use std::str::FromStr;
use tracing::Subscriber;
use tracing_subscriber::fmt::{self as tracing_fmt, format, writer::BoxMakeWriter};
use tracing_subscriber::layer::Layered;
use tracing_subscriber::registry::LookupSpan;

#[cfg(test)]
use std::io::Write;

#[cfg(test)]
use std::sync::{Arc, Mutex, OnceLock};

/// Layer type returned by the console builder.
pub type ConsoleFmtLayer<S> =
    tracing_fmt::Layer<S, format::DefaultFields, format::Format<format::Full>, BoxMakeWriter>;

/// Layer stack produced when a console layer is applied to a subscriber.
pub type ConsoleLayerStack<S> = Layered<ConsoleFmtLayer<S>, S>;

/// Where console logs should be emitted.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleOutput {
    Stdout,
    #[default]
    Stderr,
    None,
}

impl fmt::Display for ConsoleOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConsoleOutput::Stdout => write!(f, "stdout"),
            ConsoleOutput::Stderr => write!(f, "stderr"),
            ConsoleOutput::None => write!(f, "none"),
        }
    }
}

impl FromStr for ConsoleOutput {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "stdout" => Ok(ConsoleOutput::Stdout),
            "stderr" => Ok(ConsoleOutput::Stderr),
            "none" => Ok(ConsoleOutput::None),
            _ => Err(format!(
                "invalid logging.console_output '{}'; supported values are stdout, stderr, none",
                value
            )),
        }
    }
}

/// Derive the console output sink from the execution context and optional user override.
pub fn select_console_output(
    context: ExecutionContext,
    configured: Option<ConsoleOutput>,
) -> ConsoleOutput {
    match context {
        ExecutionContext::Tui => ConsoleOutput::None,
        ExecutionContext::Batch => ConsoleOutput::None,
        ExecutionContext::LocalDev => configured.unwrap_or(ConsoleOutput::Stderr),
        ExecutionContext::RemoteAgent => configured.unwrap_or(ConsoleOutput::None),
    }
}

/// Build the console tracing layer for the provided subscriber type.
pub fn console_layer<S>(output: ConsoleOutput) -> ConsoleFmtLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let make_writer = if let Some(writer) = test_override_writer() {
        writer
    } else {
        match output {
            ConsoleOutput::Stdout => BoxMakeWriter::new(io::stdout),
            ConsoleOutput::Stderr => BoxMakeWriter::new(io::stderr),
            ConsoleOutput::None => BoxMakeWriter::new(io::sink),
        }
    };

    tracing_fmt::layer()
        .with_writer(make_writer)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
}

fn test_override_writer() -> Option<BoxMakeWriter> {
    #[cfg(test)]
    {
        if let Some(slot) = TEST_OUTPUT.get() {
            if let Some(buffer) = slot.lock().unwrap().clone() {
                let buffer_clone = buffer.clone();
                return Some(BoxMakeWriter::new(move || {
                    TestGuard::new(buffer_clone.clone())
                }));
            }
        }
    }
    None
}

#[cfg(test)]
type TestOutputSlot = OnceLock<Mutex<Option<Arc<Mutex<Vec<u8>>>>>>;

#[cfg(test)]
static TEST_OUTPUT: TestOutputSlot = OnceLock::new();

#[cfg(test)]
pub fn set_test_output(buffer: Arc<Mutex<Vec<u8>>>) {
    TEST_OUTPUT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .replace(buffer);
}

#[cfg(test)]
pub fn clear_test_output() {
    if let Some(slot) = TEST_OUTPUT.get() {
        slot.lock().unwrap().take();
    }
}

#[cfg(test)]
struct TestGuard {
    buffer: Arc<Mutex<Vec<u8>>>,
}

#[cfg(test)]
impl TestGuard {
    fn new(buffer: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { buffer }
    }
}

#[cfg(test)]
impl Write for TestGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self.buffer.lock().unwrap();
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

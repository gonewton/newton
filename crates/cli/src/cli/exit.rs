//! Process-exit-code carrying error (spec 074, PR-1 / B3).
//!
//! Command handlers are reachable via three surfaces: a direct CLI
//! invocation (`newton data ...`), `newton serve --with-mcp` (MCP tool
//! calls), and the chat command surface. Only the first surface is a single
//! short-lived process where an immediate `std::process::exit` is safe; the
//! other two are long-running servers where one bad request must not take
//! the whole process down.
//!
//! `CliExit` lets a handler express "this run must terminate the process
//! with exit code N and this message" without calling `std::process::exit`
//! itself. The handler returns `Err(CliExit::new(code, message).into())`,
//! which flows through the framework's normal `anyhow::Result` dispatch:
//!
//! - Over MCP/chat, cli-framework's `tool_bridge` maps the `Err` to a
//!   `BridgeError::Execution`, which `dispatch_tool_call` turns into a
//!   structured `MCP_EXECUTION_FAILED` error frame — the server keeps
//!   running and answers the next request.
//! - For a direct CLI invocation, `main.rs` downcasts the top-level error;
//!   if it is a `CliExit`, it prints `message` to stderr and calls
//!   `std::process::exit(code)`, exactly reproducing the historical
//!   behavior of the sites this type replaces.
//!
//! Only `crates/cli/src/main.rs` may act on a `CliExit` by exiting the
//! process; every other crate/module must let it propagate as a normal
//! error.
use std::fmt;

/// An error that carries the process exit code a direct CLI invocation
/// should terminate with, without exiting the process itself.
#[derive(Debug)]
pub struct CliExit {
    /// The exit code a direct CLI invocation should terminate with.
    pub code: i32,
    /// The message a direct CLI invocation should print to stderr before
    /// exiting. Also the `Display`/error message seen by any other consumer
    /// (e.g. an MCP error frame).
    pub message: String,
}

impl CliExit {
    /// Build a `CliExit` with the given exit code and message.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for CliExit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliExit {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_renders_message_only() {
        let exit = CliExit::new(1, "DATA-001: boom");
        assert_eq!(exit.to_string(), "DATA-001: boom");
        assert_eq!(exit.code, 1);
    }

    #[test]
    fn converts_into_anyhow_and_downcasts_back() {
        let exit = CliExit::new(2, "some failure");
        let err: anyhow::Error = exit.into();
        let downcast = err
            .downcast_ref::<CliExit>()
            .expect("anyhow::Error must downcast back to CliExit");
        assert_eq!(downcast.code, 2);
        assert_eq!(downcast.message, "some failure");
    }
}

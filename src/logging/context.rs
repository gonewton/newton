use crate::cli::Command;
use std::env;

/// Execution contexts that influence how logging is routed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionContext {
    /// Interactive terminal UI such as `newton monitor`.
    Tui,
    /// Local development commands that drive a workspace from the CLI.
    LocalDev,
    /// Batch/daemon workflows that should be quiet on the console.
    Batch,
    /// Remote agent execution that runs on a different host.
    RemoteAgent,
}

impl ExecutionContext {
    /// Returns `true` when console sinks should be disabled.
    pub fn disables_console(self) -> bool {
        matches!(
            self,
            ExecutionContext::Tui | ExecutionContext::Batch | ExecutionContext::RemoteAgent
        )
    }
}

/// Derive the active execution context from a parsed CLI command plus overrides.
pub fn detect_context(command: &Command) -> ExecutionContext {
    if let Command::Monitor(_) = command {
        return ExecutionContext::Tui;
    }

    if remote_override_enabled() {
        return ExecutionContext::RemoteAgent;
    }

    match command {
        Command::Batch(_) => ExecutionContext::Batch,
        Command::Run(_)
        | Command::Step(_)
        | Command::Status(_)
        | Command::Report(_)
        | Command::Error(_)
        | Command::Init(_) => ExecutionContext::LocalDev,
        Command::Monitor(_) => ExecutionContext::Tui,
    }
}

fn remote_override_enabled() -> bool {
    env::var("NEWTON_REMOTE_AGENT")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

use crate::cli::Command;
use std::env;

/// Execution contexts used to tune logging behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionContext {
    /// Command renders a terminal UI and must avoid console logs.
    Tui,
    /// Command runs as a headless batch worker.
    Batch,
    /// Commands intended for local development/workspace interaction.
    LocalDev,
    /// Commands running on a remote agent.
    RemoteAgent,
}

/// Detect the execution context for the provided command.
pub fn detect_context(command: &Command) -> ExecutionContext {
    if remote_override_enabled() && !matches!(command, Command::Monitor(_)) {
        return ExecutionContext::RemoteAgent;
    }

    match command {
        Command::Monitor(_) => ExecutionContext::Tui,
        Command::Batch(_) => ExecutionContext::Batch,
        Command::Run(_)
        | Command::Init(_)
        | Command::Step(_)
        | Command::Status(_)
        | Command::Report(_)
        | Command::Error(_) => ExecutionContext::LocalDev,
    }
}

fn remote_override_enabled() -> bool {
    env::var("NEWTON_REMOTE_AGENT")
        .map(|val| val.trim() == "1")
        .unwrap_or(false)
}

use crate::logging::invocation::{LogInvocation, LogInvocationKind};
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
    /// API server such as `newton serve`.
    Server,
}

impl ExecutionContext {
    /// Returns `true` when console sinks should be disabled.
    pub fn disables_console(self) -> bool {
        matches!(
            self,
            ExecutionContext::Tui
                | ExecutionContext::Batch
                | ExecutionContext::RemoteAgent
                | ExecutionContext::Server
        )
    }
}

/// Derive the active execution context from a parsed CLI command plus overrides.
pub fn detect_context(command: &LogInvocation) -> ExecutionContext {
    if matches!(command.kind, LogInvocationKind::Monitor) {
        return ExecutionContext::Tui;
    }

    if remote_override_enabled() {
        return ExecutionContext::RemoteAgent;
    }

    match command.kind {
        LogInvocationKind::Batch => ExecutionContext::Batch,
        LogInvocationKind::Run
        | LogInvocationKind::Validate
        | LogInvocationKind::Dot
        | LogInvocationKind::Lint
        | LogInvocationKind::Explain
        | LogInvocationKind::Resume
        | LogInvocationKind::Checkpoints
        | LogInvocationKind::Artifacts
        | LogInvocationKind::Webhook
        | LogInvocationKind::Log
        | LogInvocationKind::Init => ExecutionContext::LocalDev,
        LogInvocationKind::Monitor => ExecutionContext::Tui,
        LogInvocationKind::Serve => ExecutionContext::Server,
    }
}

fn remote_override_enabled() -> bool {
    env::var("NEWTON_REMOTE_AGENT")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

//! Decoupled command descriptor used by core logging/integrations so the
//! library does not have to depend on the clap-derived CLI types.
//!
//! The CLI crate constructs a `LogInvocation` from its own `Command` enum and
//! passes it into [`crate::logging::init`].

use std::path::PathBuf;

/// Discriminant kind for the invoked command. Mirrors variants of the
/// CLI `Command` enum but without dragging clap-derived structs into core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogInvocationKind {
    Run,
    Init,
    Batch,
    Validate,
    Dot,
    Lint,
    Explain,
    Resume,
    Checkpoints,
    Artifacts,
    Webhook,
    Log,
    Monitor,
    Serve,
}

impl LogInvocationKind {
    /// Short stable name used by ailoop integration for the `command_name` field.
    pub fn name(&self) -> &'static str {
        match self {
            LogInvocationKind::Run => "run",
            LogInvocationKind::Init => "init",
            LogInvocationKind::Batch => "batch",
            LogInvocationKind::Validate => "validate",
            LogInvocationKind::Dot => "dot",
            LogInvocationKind::Lint => "lint",
            LogInvocationKind::Explain => "explain",
            LogInvocationKind::Resume => "resume",
            LogInvocationKind::Checkpoints => "checkpoints",
            LogInvocationKind::Artifacts => "artifacts",
            LogInvocationKind::Webhook => "webhook",
            LogInvocationKind::Log => "log",
            LogInvocationKind::Monitor => "monitor",
            LogInvocationKind::Serve => "serve",
        }
    }
}

/// Aggregated invocation info needed to bootstrap logging and ailoop
/// integration without depending on clap-derived types.
#[derive(Clone, Debug)]
pub struct LogInvocation {
    pub kind: LogInvocationKind,
    /// Best-known workspace candidate path. Logging will canonicalize and
    /// confirm `.newton/` exists; if absent, falls back to home dir.
    pub workspace_candidate: Option<PathBuf>,
}

impl LogInvocation {
    pub fn new(kind: LogInvocationKind, workspace_candidate: Option<PathBuf>) -> Self {
        Self {
            kind,
            workspace_candidate,
        }
    }
}

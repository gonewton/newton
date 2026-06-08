//! Maps a parsed cli-framework command name → [`LogInvocationKind`].
//!
//! Used by `main.rs` so that logging is initialised against the same command
//! identifier the framework will dispatch to.  Single source of truth: edits
//! here are mirrored by the `kind_for_command` unit test.

use newton_core::logging::LogInvocationKind;

pub fn kind_for_command(name: &str) -> LogInvocationKind {
    use LogInvocationKind::*;
    match name {
        "resume" => Resume,
        "init" => Init,
        "optimize" => Optimize,
        "serve" => Serve,
        "workflow" => Workflow,
        "runs" => Runs,
        "checkpoint" => Checkpoint,
        "artifact" => Artifact,
        "doctor" | "config" | "completion" | "chat" => Diagnostic,
        _ => Run,
    }
}

/// Best-effort lookup of the subcommand name from raw argv.  Returns `None`
/// when argv is too short or starts with a flag.
pub fn peek_command(argv: &[String]) -> Option<&str> {
    argv.iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(String::as_str)
}

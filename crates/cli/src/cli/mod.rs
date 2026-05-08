//! CLI scaffolding for Newton: argument parsing, command definitions, and command dispatch logic.
pub mod args;
pub mod categories;
pub mod commands;
pub mod context;
pub mod framework_setup;
pub mod init;
pub mod log_invocation;
pub mod mcp;
pub mod ops;

#[cfg(feature = "ask")]
pub mod ask;

pub use context::NewtonContext;
pub use framework_setup::build_app;
pub use log_invocation::kind_for_command;

pub use args::{
    ArtifactCommand, ArtifactsArgs, BatchArgs, CheckpointCommand, CheckpointsArgs, DotArgs,
    ExplainArgs, InitArgs, LintArgs, LogArgs, LogCommand, MonitorArgs, ResumeArgs, RunArgs,
    ServeArgs, ValidateArgs, WebhookArgs, WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};

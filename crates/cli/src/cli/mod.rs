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
pub mod workspace_paths;

pub use context::NewtonContext;
pub use framework_setup::build_app;
pub use log_invocation::kind_for_command;
pub use workspace_paths::WorkspacePaths;

pub use args::{
    ArtifactArgs, ArtifactCommand, CheckpointArgs, CheckpointCommand, DotArgs, ExplainArgs,
    GraphFormat, ImportArgs, InitArgs, LintArgs, OptimizeArgs, ResumeArgs, RunArgs, RunsArgs,
    RunsCommand, ServeArgs, ValidateArgs, WorkflowArgs, WorkflowCommand,
};

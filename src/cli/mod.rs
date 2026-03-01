//! CLI scaffolding for Newton: argument parsing, command definitions, and command dispatch logic.
pub mod args;
pub mod commands;
pub mod init;

pub use args::{
    ArtifactCommand, ArtifactsArgs, BatchArgs, CheckpointCommand, CheckpointsArgs, DotArgs,
    ExplainArgs, InitArgs, LintArgs, MonitorArgs, ResumeArgs, RunArgs, ValidateArgs, WebhookArgs,
    WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use clap::{Parser, Subcommand};

const HELP_TEMPLATE: &str = "\
{name}\n\
{about-with-newline}\n\
USAGE:\n    {usage}\n\
\nOPTIONS:\n{options}\n\
WORKFLOW COMMANDS:\n{subcommands}\n";

#[derive(Parser)]
#[command(name = "newton")]
#[command(version = crate::VERSION)]
#[command(about = "Newton Loop optimization framework in Rust")]
#[command(help_template = HELP_TEMPLATE)]
#[command(
    after_long_help = "Typical flow: run an optimization, inspect status, emit reports, then debug errors if needed."
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // Command variants mirror the CLI styling structs, so matching variants stay large by design.
pub enum Command {
    #[command(
        about = "Execute a workflow graph",
        long_about = "Run executes a workflow graph defined in YAML, with optional trigger payload from input file or arguments.",
        after_help = "Example:\n    newton run workflow.yaml input.txt --workspace ./workspace --arg key=value"
    )]
    Run(RunArgs),
    #[command(
        about = "Initialize a Newton workspace with the default template",
        long_about = "Init creates the .newton workspace layout, installs the Newton template with aikit-sdk, and writes default configs so you can run immediately.",
        after_help = "Example:\n    newton init ./workspace"
    )]
    Init(InitArgs),
    #[command(
        about = "Process queued work items for a project",
        long_about = "Batch reads plan files from .newton/plan/<project_id> and drives headless workflow orchestration.",
        after_help = "Example:\n    newton batch project-alpha --workspace ./workspace"
    )]
    Batch(BatchArgs),
    #[command(
        about = "Monitor live ailoop channels via a terminal UI",
        long_about = "Monitor listens to every project/branch channel from the workspace using a WebSocket/HTTP mix and lets you answer questions or approve authorizations in a queue.\n\n\
CONFIGURATION:\n  \
Monitor requires both HTTP and WebSocket endpoints to connect to the ailoop server.\n  \
Endpoints can come from CLI overrides (--http-url, --ws-url) or workspace config files.\n  \
Partial overrides are supported: one flag can be set while the other comes from config.\n\n\
Endpoint discovery order:\n  \
  1. CLI overrides: --http-url and --ws-url (merged with config if partial)\n  \
  2. .newton/configs/monitor.conf (if present)\n  \
  3. First alphabetical .conf file in .newton/configs/ containing both keys\n\n\
Config files use simple key=value format:\n  \
  ailoop_server_http_url = http://127.0.0.1:8081\n  \
  ailoop_server_ws_url = ws://127.0.0.1:8080",
        after_help = "EXAMPLES:\n  \
Using both CLI overrides:\n    \
newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080\n\n  \
Using .newton/configs/monitor.conf:\n    \
newton monitor\n\n  \
Partial override (HTTP from CLI, WS from config):\n    \
newton monitor --http-url http://192.168.1.10:8081\n\n\
TROUBLESHOOTING:\n  \
Missing URL configuration:\n    \
If both endpoints are not found, ensure .newton/configs/monitor.conf exists\n    \
or provide both --http-url and --ws-url on the command line.\n\n  \
Connection refused / server unavailable:\n    \
Verify the ailoop server is running at the configured endpoints.\n    \
Check URLs use correct protocol schemes (http:// and ws://).\n\n  \
Missing .newton/configs workspace setup:\n    \
Run 'newton init' in your workspace root to create the .newton directory structure,\n    \
or manually create .newton/configs/ and add a monitor.conf file."
    )]
    Monitor(MonitorArgs),
    #[command(
        about = "Validate a workflow graph definition",
        after_help = "Example:\n    newton validate workflow.yaml"
    )]
    Validate(ValidateArgs),
    #[command(
        about = "Render workflow graph as DOT",
        after_help = "Example:\n    newton dot workflow.yaml --out graph.dot"
    )]
    Dot(DotArgs),
    #[command(
        about = "Validate workflow lint rules",
        after_help = "Example:\n    newton lint workflow.yaml --format json"
    )]
    Lint(LintArgs),
    #[command(
        about = "Explain workflow graph settings/transitions",
        after_help = "Example:\n    newton explain workflow.yaml --format text"
    )]
    Explain(ExplainArgs),
    #[command(
        about = "Resume a previously-started workflow execution",
        after_help = "Example:\n    newton resume --execution-id 12345678-1234-1234-1234-123456789abc"
    )]
    Resume(ResumeArgs),
    #[command(
        about = "Inspect workflow checkpoints",
        after_help = "Example:\n    newton checkpoints list --workspace ./workspace"
    )]
    Checkpoints(CheckpointsArgs),
    #[command(
        about = "Manage workflow artifacts",
        after_help = "Example:\n    newton artifacts clean --workspace ./workspace --older-than 7d"
    )]
    Artifacts(ArtifactsArgs),
    #[command(
        about = "Manage workflow webhook listener",
        after_help = "Example:\n    newton webhook serve workflow.yaml --workspace ./workspace"
    )]
    Webhook(WebhookArgs),
}

pub async fn run(args: Args) -> crate::Result<()> {
    match args.command {
        Command::Run(run_args) => commands::run(run_args).await,
        Command::Init(init_args) => init::run(init_args).await,
        Command::Batch(batch_args) => commands::batch(batch_args).await,
        Command::Monitor(monitor_args) => commands::monitor(monitor_args).await,
        Command::Validate(validate_args) => {
            commands::validate(validate_args).map_err(anyhow::Error::from)
        }
        Command::Dot(dot_args) => commands::dot(dot_args).map_err(anyhow::Error::from),
        Command::Lint(lint_args) => commands::lint(lint_args).map_err(anyhow::Error::from),
        Command::Explain(explain_args) => {
            commands::explain(explain_args).map_err(anyhow::Error::from)
        }
        Command::Resume(resume_args) => commands::resume(resume_args)
            .await
            .map_err(anyhow::Error::from),
        Command::Checkpoints(checkpoints_args) => {
            commands::checkpoints(checkpoints_args).map_err(anyhow::Error::from)
        }
        Command::Artifacts(artifacts_args) => {
            commands::artifacts(artifacts_args).map_err(anyhow::Error::from)
        }
        Command::Webhook(webhook_args) => commands::webhook(webhook_args)
            .await
            .map_err(anyhow::Error::from),
    }
}

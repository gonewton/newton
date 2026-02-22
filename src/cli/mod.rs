//! CLI scaffolding for Newton: argument parsing, command definitions, and command dispatch logic.
pub mod args;
pub mod commands;
pub mod init;

pub use args::{
    BatchArgs, ErrorArgs, InitArgs, MonitorArgs, ReportArgs, RunArgs, StatusArgs, StepArgs,
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
        about = "Execute a full optimization loop",
        long_about = "Run iterates evaluation, advice, and execution phases until convergence or resource caps are hit.",
        after_help = "Example:\n    newton run ./workspace --max-iterations 5"
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
        long_about = "Batch reads plan files from .newton/plan/<project_id> and drives headless orchestration.",
        after_help = "Example:\n    newton batch project-alpha --workspace ./workspace"
    )]
    Batch(BatchArgs),
    #[command(
        about = "Advance loop by one cycle",
        long_about = "Step performs exactly one evaluation/advice/execution round using current workspace state.",
        after_help = "Example:\n    newton step ./workspace --execution-id exec_123"
    )]
    Step(StepArgs),
    #[command(
        about = "Inspect progress of an execution",
        long_about = "Status queries persisted artifacts for a given execution ID and surfaces current phase, iteration counts, and blockers.",
        after_help = "Example:\n    newton status exec_123 --workspace ./workspace"
    )]
    Status(StatusArgs),
    #[command(
        about = "Summarize learnings from an execution",
        long_about = "Report renders structured output (text or JSON) describing performance metrics, recommendations, and anomalies for the requested execution.",
        after_help = "Example:\n    newton report exec_123 --format json"
    )]
    Report(ReportArgs),
    #[command(
        about = "Diagnose failures during optimization",
        long_about = "Error traces tool crashes, timeouts, and incompatible artifacts for a specific execution, with optional verbose stack traces.",
        after_help = "Example:\n    newton error exec_123 --verbose"
    )]
    Error(ErrorArgs),
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
}

pub async fn run(args: Args) -> crate::Result<()> {
    match args.command {
        Command::Run(run_args) => commands::run(run_args).await,
        Command::Init(init_args) => init::run(init_args).await,
        Command::Batch(batch_args) => commands::batch(batch_args).await,
        Command::Step(step_args) => commands::step(step_args).await,
        Command::Status(status_args) => commands::status(status_args).await,
        Command::Report(report_args) => commands::report(report_args).await,
        Command::Error(error_args) => commands::error(error_args).await,
        Command::Monitor(monitor_args) => commands::monitor(monitor_args).await,
    }
}

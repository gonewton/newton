pub mod args;
pub mod commands;

use clap::{Parser, Subcommand};

const HELP_TEMPLATE: &str = "\
{name} {version}\n\
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
pub enum Command {
    #[command(
        about = "Execute a full optimization loop",
        long_about = "Run iterates evaluation, advice, and execution phases until convergence or resource caps are hit.",
        after_help = "Example:\n    newton run ./workspace --max-iterations 5"
    )]
    Run(commands::RunArgs),
    #[command(
        about = "Advance the loop by one cycle",
        long_about = "Step performs exactly one evaluation/advice/execution round using the current workspace state.",
        after_help = "Example:\n    newton step ./workspace --execution-id exec_123"
    )]
    Step(commands::StepArgs),
    #[command(
        about = "Inspect progress of an execution",
        long_about = "Status queries persisted artifacts for a given execution ID and surfaces current phase, iteration counts, and blockers.",
        after_help = "Example:\n    newton status exec_123 --workspace ./workspace"
    )]
    Status(commands::StatusArgs),
    #[command(
        about = "Summarize learnings from an execution",
        long_about = "Report renders structured output (text or JSON) describing performance metrics, recommendations, and anomalies for the requested execution.",
        after_help = "Example:\n    newton report exec_123 --format json"
    )]
    Report(commands::ReportArgs),
    #[command(
        about = "Diagnose failures during optimization",
        long_about = "Error traces tool crashes, timeouts, and incompatible artifacts for a specific execution, with optional verbose stack traces.",
        after_help = "Example:\n    newton error exec_123 --verbose"
    )]
    Error(commands::ErrorArgs),
}

pub async fn run(args: Args) -> crate::Result<()> {
    match args.command {
        Command::Run(run_args) => commands::run(run_args).await,
        Command::Step(step_args) => commands::step(step_args).await,
        Command::Status(status_args) => commands::status(status_args).await,
        Command::Report(report_args) => commands::report(report_args).await,
        Command::Error(error_args) => commands::error(error_args).await,
    }
}

pub mod args;
pub mod commands;

use clap::Parser;

const HELP_TEMPLATE: &str =
    "{name} {version}\n{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}";

#[derive(Parser)]
#[command(name = "newton")]
#[command(version = crate::VERSION)]
#[command(about = "Newton Loop optimization framework in Rust")]
#[command(help_template = HELP_TEMPLATE)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Parser)]
pub enum Command {
    Run(commands::RunArgs),
    Step(commands::StepArgs),
    Status(commands::StatusArgs),
    Report(commands::ReportArgs),
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

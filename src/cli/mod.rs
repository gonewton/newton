pub mod args;
pub mod commands;

use clap::Parser;

#[derive(Parser)]
#[command(name = "newton-code")]
#[command(about = "Newton Loop optimization framework in Rust")]
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
}

pub async fn run(args: Args) -> crate::Result<()> {
    match args.command {
        Command::Run(run_args) => commands::run(run_args).await,
        Command::Step(step_args) => commands::step(step_args).await,
        Command::Status(status_args) => commands::status(status_args).await,
        Command::Report(report_args) => commands::report(report_args).await,
    }
}
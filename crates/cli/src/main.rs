use clap::Parser;
use newton_cli::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = newton_cli::cli::Args::parse();
    let log_invocation = newton_cli::cli::log_invocation_from_command(&args.command);
    let _log_guard = newton_core::logging::init(&log_invocation, args.log_dir.as_deref())?;
    newton_cli::cli::run(args).await
}

use clap::Parser;
use newton::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = newton::cli::Args::parse();

    // Initialize logging (must happen once per process)
    let _log_guard = newton::logging::init(&args.command, args.log_dir.as_deref())?;

    // Run the chosen command
    newton::cli::run(args).await
}

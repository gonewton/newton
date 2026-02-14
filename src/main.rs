use clap::Parser;
use newton::{cli, logging, Result};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = cli::Args::parse();

    // Initialize logging once globally
    let _logging_guard = logging::init(&args.command)?;

    // Run the command
    cli::run(args).await
}
// Test auto-release workflow

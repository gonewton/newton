use clap::Parser;
use newton_code::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse CLI arguments
    let args = newton_code::cli::Args::parse();

    // Run the command
    newton_code::cli::run(args).await
}
pub mod cli;
pub mod core;
pub mod logging;
pub mod monitor;
pub mod tools;
pub mod utils;

/// Current crate version string exposed for CLI and tests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Result<T> = std::result::Result<T, anyhow::Error>;

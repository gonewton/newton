//! Newton core library.
pub mod api;
pub mod core;
pub mod integrations;
pub mod logging;
pub mod utils;
pub mod workflow;

/// Current crate version string exposed for CLI and tests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Result<T> = std::result::Result<T, anyhow::Error>;

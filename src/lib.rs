//! Newton CLI crate re-exports for orchestrating AI-driven optimization loops and exposing the shared API surface.
pub mod api;
pub mod cli;
pub mod core;
pub mod integrations;
pub mod logging;
pub mod monitor;
pub mod utils;
pub mod workflow;

/// Current crate version string exposed for CLI and tests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Result<T> = std::result::Result<T, anyhow::Error>;

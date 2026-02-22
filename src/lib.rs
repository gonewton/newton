//! Newton CLI crate re-exports for orchestrating AI-driven optimization loops and exposing the shared API surface.
pub mod ailoop_integration;
pub mod cli;
pub mod core;
pub mod logging;
pub mod monitor;
pub mod tools;
pub mod utils;

/// Current crate version string exposed for CLI and tests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Result<T> = std::result::Result<T, anyhow::Error>;

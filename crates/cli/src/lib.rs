//! Newton CLI library.
pub mod cli;
pub mod monitor;

pub use newton_core::Result;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

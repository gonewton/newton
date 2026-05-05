//! Newton CLI library.
pub mod cli;
pub mod monitor;

pub use cli::context::NewtonContext;
pub use cli::framework_setup::build_app;
pub use newton_core::Result;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

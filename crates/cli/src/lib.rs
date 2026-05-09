//! Newton CLI library.
pub mod cli;

pub use cli::context::NewtonContext;
pub use cli::framework_setup::build_app;
pub use cli::log_invocation::kind_for_command;
pub use cli::ops;

#[cfg(feature = "ask")]
pub use cli::ask;

pub use newton_core::Result;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

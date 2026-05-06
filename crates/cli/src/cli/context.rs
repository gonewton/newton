//! Newton CLI context for framework-driven command dispatch (Issue #228 Stage 2).

use cli_framework::app::AppContext;

/// Shared context handed to every command execute closure.
/// Holds no mutable, command-specific state — logging is initialised in main
/// before the framework runs, so no guard handle is needed here.
#[derive(Default)]
pub struct NewtonContext {
    _private: (),
}

impl NewtonContext {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl AppContext for NewtonContext {}

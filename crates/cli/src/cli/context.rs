//! Newton CLI context for framework-driven command dispatch (Issue #228 Stage 2).
//!
//! `NewtonContext` is the shared, mutation-free state container that future
//! `cli-framework` `App<NewtonContext>` execute closures will receive. It is
//! introduced now as scaffolding so the migration referenced by the
//! `feature/228-054-migrate-cli-framework` spec can land in stages without
//! breaking the existing clap-based dispatch path.

/// Shared context handed to every command execute closure once the migration
/// to `cli-framework` is wired up. Holds no mutable, command-specific state.
#[derive(Default)]
pub struct NewtonContext {
    _private: (),
}

impl NewtonContext {
    /// Construct a fresh `NewtonContext`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

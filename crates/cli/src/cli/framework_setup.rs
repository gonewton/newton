//! Framework registration scaffolding for the Newton CLI (Issue #228 Stage 2).
//!
//! This module is the eventual home of `cli-framework` `Command` /
//! `CommandSpec` / `ArgSpec` declarations and the `build_app()` entry point
//! used by `crates/cli/src/main.rs`. The full migration is staged across
//! multiple deliverables (see spec §10); this file currently provides the
//! public API surface so dependent crates and tests can compile against the
//! new boundary while the underlying implementation continues to use the
//! existing clap-driven dispatch defined in `crate::cli::run`.
//!
//! Once `cli-framework` is added as a workspace dependency, the body of
//! `build_app` will be replaced by `AppBuilder::new()...build(ctx)` and the
//! per-command `Command` registrations enumerated in §5 of the spec.

use crate::cli::context::NewtonContext;
use crate::Result;

/// Error codes raised by the framework migration adapter layer.
///
/// See spec §6 for the full taxonomy.
pub mod error_codes {
    /// `AppBuilder::build()` is missing a currently-supported Newton command.
    pub const CLI_MIG_001: &str = "CLI-MIG-001";
    /// `TryFrom<CommandArgs>` adapter cannot construct a required Newton DTO field.
    pub const CLI_MIG_002: &str = "CLI-MIG-002";
    /// Framework parsing accepts ambiguous or conflicting flags.
    pub const CLI_MIG_003: &str = "CLI-MIG-003";
    /// Help-output contract check fails for a migrated command.
    pub const CLI_MIG_004: &str = "CLI-MIG-004";
    /// Framework parser returned a command path with no registered execute closure.
    pub const CLI_MIG_005: &str = "CLI-MIG-005";
}

/// Newton CLI app handle returned by [`build_app`].
///
/// During the staged migration this is a thin wrapper that defers to the
/// existing clap-based [`crate::cli::run`] dispatch. Once Stage 5 lands it
/// will hold a `cli_framework::App<NewtonContext>` and surface the framework
/// directly.
pub struct NewtonApp {
    _ctx: NewtonContext,
}

impl NewtonApp {
    /// Drive the CLI to completion using the current dispatch path.
    pub async fn run(self) -> Result<()> {
        use clap::Parser;
        let parsed = crate::cli::Args::parse();
        crate::cli::run(parsed).await
    }
}

/// Build the Newton CLI application with framework-style entry semantics.
///
/// This is the function `crates/cli/src/main.rs` will eventually call once
/// the cli-framework dependency is wired in. Today it returns a wrapper that
/// runs the existing clap-based dispatch so the public API surface stays
/// stable across the staged migration.
pub fn build_app(ctx: NewtonContext) -> Result<NewtonApp> {
    Ok(NewtonApp { _ctx: ctx })
}

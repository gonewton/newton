//! Declarative optimization policy, independent of agents and concrete storage.
//!
//! The native driver persists bound definitions/revisions and enforces the
//! capabilities supplied here. These pure helpers do not sandbox executables,
//! dispatch work, merge changes, or claim durable acknowledgment on their own.

mod binding;
mod decision;
mod outcome;
mod revisions;
mod schema;
mod validation;

pub use binding::*;
pub use decision::*;
pub use outcome::*;
pub use revisions::*;
pub use schema::*;
pub use validation::{parse_definition, validate_definition, validate_requirements};

/// Invalid policy, authority, or evidence; never a successful convergence signal.
#[derive(Debug, thiserror::Error)]
pub enum OptimizationError {
    /// Definition parsing or semantic validation failed.
    #[error("invalid optimization definition: {0}")]
    InvalidDefinition(String),
    /// Run/context identifiers or parameter bindings are invalid.
    #[error("invalid optimization binding: {0}")]
    InvalidBinding(String),
    /// The host cannot enforce a declared action or protected-path restriction.
    #[error("unsupported execution restriction: {0}")]
    UnsupportedRestriction(String),
    /// A trusted control caller lacks required update/action permissions.
    #[error("optimization authority denied: {0}")]
    Unauthorized(String),
    /// Another update became active before this request was acknowledged.
    #[error("stale requirements revision {actual}; active revision is {expected}")]
    StaleRevision { expected: u64, actual: u64 },
    /// Evidence belongs to another state, evaluator, or requirements revision.
    #[error("invalid optimization evidence: {0}")]
    InvalidEvidence(String),
    /// A requested result/report would contradict the actual policy decision.
    #[error("invalid optimization outcome: {0}")]
    InvalidOutcome(String),
}

#[cfg(test)]
mod tests;

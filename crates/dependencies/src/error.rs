use thiserror::Error;

/// Invalid catalog facts, discovery inputs, or baseline approval.
#[derive(Debug, Error)]
pub enum DependencyError {
    /// An artifact identity is empty or appears more than once.
    #[error("artifact identity is empty or duplicated: {0}")]
    InvalidIdentity(String),
    /// A relationship references an absent artifact.
    #[error("unknown artifact: {0}")]
    UnknownArtifact(String),
    /// Membership does not follow Product -> Component -> Repo -> Module.
    #[error("invalid hierarchy membership: {0}")]
    InvalidMembership(String),
    /// An edge or its constraint cannot be interpreted without guessing.
    #[error("invalid dependency: {0}")]
    InvalidDependency(String),
    /// Baseline authorization or its exact reviewed map is missing.
    #[error("baseline approval rejected: {0}")]
    InvalidApproval(String),
    /// The input is not a supported Cargo manifest.
    #[error("invalid Cargo manifest: {0}")]
    InvalidManifest(String),
    /// The changed artifact must be a Module.
    #[error("changed artifact must be a Module: {0}")]
    InvalidChange(String),
    /// Canonical serialization failed.
    #[error("could not serialize dependency map: {0}")]
    Serialization(#[from] serde_json::Error),
}

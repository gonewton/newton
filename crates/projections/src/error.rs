use thiserror::Error;

/// Projection-only failures; callers must not reinterpret these as work decisions.
#[derive(Debug, Error)]
pub enum ProjectionError {
    /// A required internal identity or configured target is invalid.
    #[error("invalid projection input: {0}")]
    Invalid(String),
    /// Delivery requires a previously persisted external binding.
    #[error("projection has no durable binding")]
    Unbound,
    /// Existing identities cannot be silently rebound to another resource.
    #[error("projection is already bound to a different target")]
    BindingConflict,
    /// The same entity revision was reused with a different status.
    #[error("projection revision {0} has conflicting status payloads")]
    RevisionConflict(u64),
    /// Another process currently owns delivery for this entity/destination.
    #[error("projection has another active delivery owner")]
    Busy,
    /// Adapter failure, safe to retry only under its idempotent-assignment contract.
    #[error("projection delivery failed: {0}")]
    Delivery(String),
    /// Local durability failure; no successful-delivery claim may be returned.
    #[error("projection journal I/O failure: {0}")]
    Io(#[from] std::io::Error),
    /// Invalid or unreadable persisted data.
    #[error("projection journal serialization failure: {0}")]
    Serialization(#[from] serde_json::Error),
}

use crate::{ProjectionBinding, ProjectionError, ProjectionReceipt, ProjectionUpdate};
use async_trait::async_trait;

/// Write-only external projection. It cannot enumerate or read external work state.
///
/// Implementations MUST assign state on an existing bound resource idempotently:
/// an ambiguous failure may be retried with the same binding/update. Resource
/// creation, comments, or other append-only writes require a different contract.
#[async_trait]
pub trait ProjectionPort: Send + Sync {
    /// Reflect Newton-owned status without returning any domain scheduling input.
    async fn reflect(
        &self,
        binding: &ProjectionBinding,
        update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError>;
}

/// Local-only mode; no tracker, credentials, transport, or journal is required.
#[derive(Debug, Default)]
pub struct NoTracker;

#[async_trait]
impl ProjectionPort for NoTracker {
    async fn reflect(
        &self,
        binding: &ProjectionBinding,
        _update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError> {
        if !matches!(binding, ProjectionBinding::None) {
            return Err(ProjectionError::Invalid(
                "NoTracker cannot acknowledge an external binding".into(),
            ));
        }
        Ok(ProjectionReceipt { external_ref: None })
    }
}

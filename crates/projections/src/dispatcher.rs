use crate::{
    DeliveryOutcome, ProjectionBinding, ProjectionError, ProjectionKey, ProjectionLease,
    ProjectionPort, ProjectionRecord, ProjectionStore, ProjectionUpdate,
};

/// Durable, optional delivery coordinator; it has no access to optimizer work state.
pub struct ProjectionDispatcher<S> {
    store: S,
}

impl<S: ProjectionStore> ProjectionDispatcher<S> {
    /// Compose the journal without configuring or requiring any external tracker.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Persist an explicitly authorized binding before external writes can occur.
    /// Repeating the same binding is idempotent; silent rebinding is rejected.
    pub fn bind(
        &self,
        key: &ProjectionKey,
        binding: ProjectionBinding,
    ) -> Result<(), ProjectionError> {
        let lease = self.store.acquire(key)?;
        if let Some(record) = lease.load()? {
            return if record.binding == binding {
                Ok(())
            } else {
                Err(ProjectionError::BindingConflict)
            };
        }
        validate_binding(&binding)?;
        lease.save(&ProjectionRecord {
            key: key.clone(),
            binding,
            delivered: None,
            pending: None,
            last_error: None,
        })
    }

    /// Read only Newton-owned delivery metadata, never the external board.
    pub fn record(&self, key: &ProjectionKey) -> Result<Option<ProjectionRecord>, ProjectionError> {
        self.store.acquire(key)?.load()
    }

    /// Persist a revision before assigning external status, then persist completion.
    ///
    /// A lease spans the external call so an older delivery cannot race a newer
    /// one. Adapter outages return Deferred; storage failures stay explicit errors.
    /// A failed completion write leaves a replayable idempotent pending assignment.
    pub async fn deliver(
        &self,
        key: &ProjectionKey,
        update: ProjectionUpdate,
        port: &dyn ProjectionPort,
    ) -> Result<DeliveryOutcome, ProjectionError> {
        if update.derived_status.trim().is_empty() {
            return Err(ProjectionError::Invalid(
                "derived status must not be empty".into(),
            ));
        }
        let lease = self.store.acquire(key)?;
        let mut record = lease.load()?.ok_or(ProjectionError::Unbound)?;
        let current = record.pending.as_ref().or(record.delivered.as_ref());
        if let Some(current) = current {
            if update.revision < current.revision {
                return Ok(DeliveryOutcome::Superseded {
                    current_revision: current.revision,
                });
            }
            if update.revision == current.revision && update != *current {
                return Err(ProjectionError::RevisionConflict(update.revision));
            }
        }
        if record.pending.is_none() && record.delivered.as_ref() == Some(&update) {
            return Ok(DeliveryOutcome::AlreadyDelivered {
                revision: update.revision,
            });
        }
        record.pending = Some(update);
        record.last_error = None;
        lease.save(&record)?;
        dispatch_pending(lease.as_ref(), record, port).await
    }

    /// Replay the stored assignment after outage or crash, using its durable link.
    pub async fn retry_pending(
        &self,
        key: &ProjectionKey,
        port: &dyn ProjectionPort,
    ) -> Result<DeliveryOutcome, ProjectionError> {
        let lease = self.store.acquire(key)?;
        let record = lease.load()?.ok_or(ProjectionError::Unbound)?;
        dispatch_pending(lease.as_ref(), record, port).await
    }
}

async fn dispatch_pending(
    lease: &dyn ProjectionLease,
    mut record: ProjectionRecord,
    port: &dyn ProjectionPort,
) -> Result<DeliveryOutcome, ProjectionError> {
    let Some(update) = record.pending.clone() else {
        return Ok(DeliveryOutcome::Idle);
    };
    match port.reflect(&record.binding, &update).await {
        Ok(receipt) => {
            record.delivered = Some(update.clone());
            record.pending = None;
            record.last_error = None;
            lease.save(&record)?;
            Ok(DeliveryOutcome::Delivered {
                revision: update.revision,
                external_ref: receipt.external_ref,
            })
        }
        Err(error) => {
            let error = error.to_string();
            record.last_error = Some(error.clone());
            lease.save(&record)?;
            Ok(DeliveryOutcome::Deferred {
                revision: update.revision,
                error,
            })
        }
    }
}

fn validate_binding(binding: &ProjectionBinding) -> Result<(), ProjectionError> {
    if let ProjectionBinding::GithubProjectItem {
        project_id,
        item_id,
        field_id,
        status_options,
    } = binding
    {
        if [project_id, item_id, field_id]
            .iter()
            .any(|id| id.trim().is_empty())
            || status_options.is_empty()
            || status_options
                .iter()
                .any(|(status, id)| status.trim().is_empty() || id.trim().is_empty())
        {
            return Err(ProjectionError::Invalid(
                "existing Project/item/field IDs and nonempty status option mappings are required"
                    .into(),
            ));
        }
    }
    Ok(())
}

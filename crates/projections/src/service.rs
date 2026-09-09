//! Bounded lifecycle delivery, consuming only immutable Newton-owned snapshots.

use crate::{
    DeliveryOutcome, ProjectionBinding, ProjectionDispatcher, ProjectionError, ProjectionKey,
    ProjectionPort, ProjectionReceipt, ProjectionStore, ProjectionUpdate,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::Instant;

/// Optional local configuration frozen in the Optimize Run journal before work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunProjectionConfiguration {
    /// Declarative wire version; currently `1`.
    pub schema_version: u32,
    /// Existing tracker destinations for this run's status; no resource creation.
    pub destinations: Vec<RunProjectionDestination>,
    /// Maximum target delivery attempts per lifecycle synchronization.
    #[serde(default = "default_max_attempts")]
    pub max_delivery_attempts: usize,
    /// Whole synchronization's outbound-call budget, shared by all targets.
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_max_attempts() -> usize {
    8
}
fn default_timeout() -> u64 {
    5
}

/// A stable local destination name and an explicitly bound existing resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunProjectionDestination {
    /// Stable identity used in the durable delivery key.
    pub name: String,
    /// Existing Project/item/field identities and approved status mapping.
    pub binding: ProjectionBinding,
}

/// Immutable input derived from a durably recorded Optimize Run transition.
/// No tracker data is accepted as an input or returned as a scheduling decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProjectionSnapshot {
    /// Canonical Optimize Run identity.
    pub run_id: String,
    /// Monotonic transition revision supplied by the native lifecycle.
    pub revision: u64,
    /// Authoritative Newton status, not a status read from the tracker.
    pub status: String,
}

/// Per-destination diagnostics separate from optimization success and acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetProjectionReport {
    /// Configured destination identity.
    pub destination: String,
    /// Successful, duplicate, superseded, or durably deferred delivery.
    pub delivery: Option<DeliveryOutcome>,
    /// Journal/configuration error; never a successful-delivery claim.
    pub error: Option<String>,
}

/// Observation-only report; it contains no next-work, approval, or acceptance fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunProjectionReport {
    /// False for an absent or empty configuration; no tracker is required.
    pub configured: bool,
    /// Delivery result per configured destination.
    pub targets: Vec<TargetProjectionReport>,
    /// Projection-only diagnostics, including rejected authority or configuration.
    pub diagnostics: Vec<String>,
}

impl RunProjectionConfiguration {
    /// Parse strict declarative JSON without executing or resolving any references.
    pub fn parse(source: &str) -> Result<Self, ProjectionError> {
        let config: Self = serde_json::from_str(source)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate finite delivery limits and unique, explicitly bound destinations.
    pub fn validate(&self) -> Result<(), ProjectionError> {
        if self.schema_version != 1
            || !(1..=64).contains(&self.max_delivery_attempts)
            || !(1..=60).contains(&self.timeout_seconds)
            || self.destinations.len() > 64
        {
            return Err(ProjectionError::Invalid("schema_version must be 1; at most 64 destinations and 1–64 attempts are supported; timeout_seconds must be 1–60".into()));
        }
        let mut names = BTreeSet::new();
        for destination in &self.destinations {
            if destination.name.trim().is_empty() || !names.insert(&destination.name) {
                return Err(ProjectionError::Invalid(
                    "projection destination names must be nonempty and unique".into(),
                ));
            }
            if matches!(destination.binding, ProjectionBinding::None) {
                return Err(ProjectionError::Invalid(
                    "omit a destination instead of binding it to None".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Native-lifecycle adapter service with durable idempotency and bounded transport.
/// It has no reference to the optimizer's mutable state or its work-selection API.
pub struct RunProjectionService<S> {
    dispatcher: ProjectionDispatcher<S>,
    configuration: RunProjectionConfiguration,
    run_id: String,
    port: Arc<dyn ProjectionPort>,
}

impl<S: ProjectionStore> RunProjectionService<S> {
    /// Compose a previously frozen configuration and explicit host write authority.
    /// Bindings are persisted before any external write. No network call is made.
    pub fn new(
        run_id: impl Into<String>,
        configuration: RunProjectionConfiguration,
        write_authorized: bool,
        store: S,
        port: Arc<dyn ProjectionPort>,
    ) -> Result<Self, ProjectionError> {
        configuration.validate()?;
        let run_id = run_id.into();
        if run_id.trim().is_empty() {
            return Err(ProjectionError::Invalid(
                "Optimize Run identity is required".into(),
            ));
        }
        if !configuration.destinations.is_empty() && !write_authorized {
            return Err(ProjectionError::Invalid(
                "tracker projection requires explicit host publication authority".into(),
            ));
        }
        let service = Self {
            dispatcher: ProjectionDispatcher::new(store),
            configuration,
            run_id,
            port,
        };
        for destination in &service.configuration.destinations {
            service
                .dispatcher
                .bind(&service.key(&destination.name), destination.binding.clone())?;
        }
        Ok(service)
    }

    /// Reflect a persisted internal snapshot. Outage, timeout, and journal errors
    /// are reported separately and cannot become optimizer failures or decisions.
    /// Repeating this call retries durable pending state without creating resources.
    pub async fn reflect(&self, snapshot: &RunProjectionSnapshot) -> RunProjectionReport {
        let mut report = RunProjectionReport {
            configured: !self.configuration.destinations.is_empty(),
            ..Default::default()
        };
        if snapshot.run_id != self.run_id
            || snapshot.status.trim().is_empty()
            || snapshot.revision == 0
        {
            report.diagnostics.push(
                "projection snapshot has an invalid run identity, revision, or status".into(),
            );
            return report;
        }
        let deadline = Instant::now() + Duration::from_secs(self.configuration.timeout_seconds);
        let remaining_attempts =
            Arc::new(AtomicUsize::new(self.configuration.max_delivery_attempts));
        for destination in &self.configuration.destinations {
            let port = BoundedPort {
                inner: self.port.as_ref(),
                deadline,
                remaining_attempts: remaining_attempts.clone(),
            };
            let result = self
                .dispatcher
                .deliver(
                    &self.key(&destination.name),
                    ProjectionUpdate {
                        revision: snapshot.revision,
                        derived_status: snapshot.status.clone(),
                    },
                    &port,
                )
                .await;
            let (delivery, error) = match result {
                Ok(delivery) => (Some(delivery), None),
                Err(error) => (None, Some(error.to_string())),
            };
            report.targets.push(TargetProjectionReport {
                destination: destination.name.clone(),
                delivery,
                error,
            });
        }
        report
    }

    /// Retry already persisted assignments without reading or guessing remote state.
    /// This is safe after an outage or response loss, including after process restart.
    pub async fn retry_pending(&self) -> RunProjectionReport {
        let mut report = RunProjectionReport {
            configured: !self.configuration.destinations.is_empty(),
            ..Default::default()
        };
        let deadline = Instant::now() + Duration::from_secs(self.configuration.timeout_seconds);
        let remaining_attempts =
            Arc::new(AtomicUsize::new(self.configuration.max_delivery_attempts));
        for destination in &self.configuration.destinations {
            let port = BoundedPort {
                inner: self.port.as_ref(),
                deadline,
                remaining_attempts: remaining_attempts.clone(),
            };
            let result = self
                .dispatcher
                .retry_pending(&self.key(&destination.name), &port)
                .await;
            let (delivery, error) = match result {
                Ok(delivery) => (Some(delivery), None),
                Err(error) => (None, Some(error.to_string())),
            };
            report.targets.push(TargetProjectionReport {
                destination: destination.name.clone(),
                delivery,
                error,
            });
        }
        report
    }

    fn key(&self, destination: &str) -> ProjectionKey {
        ProjectionKey {
            entity_kind: "optimize_run".into(),
            entity_id: self.run_id.clone(),
            destination: destination.into(),
        }
    }
}

struct BoundedPort<'a> {
    inner: &'a dyn ProjectionPort,
    deadline: Instant,
    remaining_attempts: Arc<AtomicUsize>,
}

#[async_trait]
impl ProjectionPort for BoundedPort<'_> {
    async fn reflect(
        &self,
        binding: &ProjectionBinding,
        update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError> {
        if Instant::now() >= self.deadline
            || self
                .remaining_attempts
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_err()
        {
            return Err(ProjectionError::Delivery(
                "projection delivery budget exhausted; assignment remains pending".into(),
            ));
        }
        tokio::time::timeout_at(self.deadline, self.inner.reflect(binding, update))
            .await
            .map_err(|_| {
                ProjectionError::Delivery(
                    "projection delivery timed out; the same assignment remains pending for retry"
                        .into(),
                )
            })?
    }
}

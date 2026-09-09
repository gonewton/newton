//! Same-process, read-only Optimize Run observation with durable lag recovery.

use newton_types::{BackendStore, BroadcastEvent, OptimizeRunTrajectory};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Observation failures are not optimizer work, acceptance, or completion decisions.
#[derive(Debug, thiserror::Error)]
pub enum OptimizationObservationError {
    /// A scoped snapshot could not be read from the authoritative store.
    #[error("cannot read Optimize Run observation: {0}")]
    Store(String),
    /// A faulty store returned data belonging to a different run.
    #[error("Optimize Run observation returned out-of-scope data")]
    ScopeMismatch,
    /// The requested identity was empty.
    #[error("Optimize Run observation requires a nonempty run identity")]
    MissingRun,
    /// Concurrent writes prevented a coherent snapshot within the bounded retries.
    #[error("Optimize Run changed while reading its snapshot; retry observation")]
    SnapshotBusy,
}

/// Reason a fresh scoped snapshot was supplied; no other run's event metadata leaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunObservationReason {
    /// The real native driver published a durable change for this run.
    Changed,
    /// The bounded channel overflowed; the snapshot recovers current durable state.
    LagRecovered,
    /// An embedding caller explicitly requested a fresh snapshot.
    Refreshed,
}

/// Latest durable Run/Cycle snapshot, not an inferred delta or synthetic event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunObservationUpdate {
    /// Why this snapshot was fetched.
    pub reason: RunObservationReason,
    /// Only the bound Run and its persisted Cycles.
    pub snapshot: OptimizeRunTrajectory,
}

/// Compose the exact store and publisher used by the native driver.
///
/// This is an in-process read capability, not a server or authentication bypass.
/// An embedding application must authorize the run identity before subscription.
/// Existing HTTP authentication/exposure rules remain unchanged.
#[derive(Clone)]
pub struct OptimizeRunObservationSource {
    store: Arc<dyn BackendStore>,
    publisher: broadcast::Sender<BroadcastEvent>,
}

impl OptimizeRunObservationSource {
    /// Reuse the driver's actual store and event channel; do not create a second
    /// publisher in another process and expect it to receive native events.
    pub fn new(store: Arc<dyn BackendStore>, publisher: broadcast::Sender<BroadcastEvent>) -> Self {
        Self { store, publisher }
    }

    /// Subscribe before reading the initial snapshot, closing the usual
    /// snapshot-then-subscribe lost-update window. No task, daemon, or socket starts.
    pub async fn subscribe(
        &self,
        run_id: &str,
    ) -> Result<OptimizeRunObservation, OptimizationObservationError> {
        if run_id.trim().is_empty() {
            return Err(OptimizationObservationError::MissingRun);
        }
        let receiver = self.publisher.subscribe();
        let snapshot = read_snapshot(self.store.as_ref(), run_id).await?;
        Ok(OptimizeRunObservation {
            run_id: run_id.into(),
            store: self.store.clone(),
            receiver,
            snapshot,
            pending: None,
        })
    }
}

/// Run-scoped read-only observer. Dropping it never cancels or changes the run.
/// The channel is bounded by the driver; snapshots read the full persisted
/// trajectory and therefore cost O(number of recorded Cycles).
pub struct OptimizeRunObservation {
    run_id: String,
    store: Arc<dyn BackendStore>,
    receiver: broadcast::Receiver<BroadcastEvent>,
    snapshot: OptimizeRunTrajectory,
    pending: Option<RunObservationReason>,
}

impl OptimizeRunObservation {
    /// Initial snapshot, or the most recently delivered durable snapshot.
    pub fn snapshot(&self) -> &OptimizeRunTrajectory {
        &self.snapshot
    }

    /// Wait for this run's actual driver event, filtering other runs and unrelated
    /// events. Lag automatically recovers from storage instead of silently dropping
    /// updates. `None` means all publishers closed, not optimizer completion.
    /// A failed or cancelled snapshot read stays pending for the next call.
    pub async fn next_update(
        &mut self,
    ) -> Result<Option<RunObservationUpdate>, OptimizationObservationError> {
        loop {
            if let Some(reason) = self.pending {
                return self.read_update(reason).await.map(Some);
            }
            let reason = match self.receiver.recv().await {
                Ok(BroadcastEvent::OptimizeRunUpdate { run_id, .. }) if run_id == self.run_id => {
                    RunObservationReason::Changed
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => RunObservationReason::LagRecovered,
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
            };
            self.pending = Some(reason);
        }
    }

    /// Explicitly refresh a scoped snapshot, for example after reconnecting an
    /// embedding consumer. This does not read or alter any external tracker.
    pub async fn refresh(&mut self) -> Result<RunObservationUpdate, OptimizationObservationError> {
        self.read_update(RunObservationReason::Refreshed).await
    }

    async fn read_update(
        &mut self,
        reason: RunObservationReason,
    ) -> Result<RunObservationUpdate, OptimizationObservationError> {
        self.snapshot = read_snapshot(self.store.as_ref(), &self.run_id).await?;
        self.pending = None;
        Ok(RunObservationUpdate {
            reason,
            snapshot: self.snapshot.clone(),
        })
    }
}

async fn read_snapshot(
    store: &dyn BackendStore,
    run_id: &str,
) -> Result<OptimizeRunTrajectory, OptimizationObservationError> {
    // BackendStore does not expose a read transaction. Compare surrounding Run
    // reads and retry a bounded number of times rather than mix two transitions.
    for _ in 0..4 {
        let before = store.get_optimize_run(run_id).await.map_err(store_error)?;
        let mut cycles = store
            .list_optimize_cycles(run_id)
            .await
            .map_err(store_error)?;
        let after = store.get_optimize_run(run_id).await.map_err(store_error)?;
        if before.run.id != run_id
            || after.run.id != run_id
            || cycles.iter().any(|cycle| cycle.run_id != run_id)
        {
            return Err(OptimizationObservationError::ScopeMismatch);
        }
        let before_value = serde_json::to_value(&before)
            .map_err(|error| OptimizationObservationError::Store(error.to_string()))?;
        let after_value = serde_json::to_value(&after)
            .map_err(|error| OptimizationObservationError::Store(error.to_string()))?;
        if before_value == after_value && cycles.iter().all(|cycle| cycle.cycle <= after.run.cycle)
        {
            cycles.sort_by(|left, right| {
                left.cycle
                    .cmp(&right.cycle)
                    .then_with(|| left.id.cmp(&right.id))
            });
            return Ok(OptimizeRunTrajectory {
                detail: after,
                cycles,
            });
        }
        tokio::task::yield_now().await;
    }
    Err(OptimizationObservationError::SnapshotBusy)
}

fn store_error(error: newton_types::ApiError) -> OptimizationObservationError {
    OptimizationObservationError::Store(format!("{}: {}", error.code, error.message))
}

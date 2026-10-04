//! Same-process, read-only observation of authoritative optimization JSON files.

use newton_types::{optimization::*, BroadcastEvent};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum OptimizationObservationError {
    #[error("cannot read Optimize Run observation: {0}")]
    Store(String),
    #[error("Optimize Run observation returned out-of-scope data")]
    ScopeMismatch,
    #[error("Optimize Run observation requires a nonempty run identity")]
    MissingRun,
    #[error("Optimize Run changed while reading its snapshot; retry observation")]
    SnapshotBusy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunObservationReason {
    Changed,
    LagRecovered,
    Refreshed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunObservationUpdate {
    pub reason: RunObservationReason,
    pub snapshot: OptimizationRunSnapshot,
}

#[derive(Clone)]
pub struct OptimizeRunObservationSource {
    state_dir: PathBuf,
    publisher: broadcast::Sender<BroadcastEvent>,
}

impl OptimizeRunObservationSource {
    pub fn new(state_dir: PathBuf, publisher: broadcast::Sender<BroadcastEvent>) -> Self {
        Self {
            state_dir,
            publisher,
        }
    }

    pub async fn subscribe(
        &self,
        run_id: &str,
    ) -> Result<OptimizeRunObservation, OptimizationObservationError> {
        if run_id.trim().is_empty() {
            return Err(OptimizationObservationError::MissingRun);
        }
        let receiver = self.publisher.subscribe();
        let snapshot = read_snapshot(&self.state_dir, run_id)?;
        Ok(OptimizeRunObservation {
            run_id: run_id.into(),
            state_dir: self.state_dir.clone(),
            receiver,
            snapshot,
            pending: None,
        })
    }
}

pub struct OptimizeRunObservation {
    run_id: String,
    state_dir: PathBuf,
    receiver: broadcast::Receiver<BroadcastEvent>,
    snapshot: OptimizationRunSnapshot,
    pending: Option<RunObservationReason>,
}

impl OptimizeRunObservation {
    pub fn snapshot(&self) -> &OptimizationRunSnapshot {
        &self.snapshot
    }

    pub async fn next_update(
        &mut self,
    ) -> Result<Option<RunObservationUpdate>, OptimizationObservationError> {
        loop {
            if let Some(reason) = self.pending {
                return self.read_update(reason).map(Some);
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

    pub async fn refresh(&mut self) -> Result<RunObservationUpdate, OptimizationObservationError> {
        self.read_update(RunObservationReason::Refreshed)
    }

    fn read_update(
        &mut self,
        reason: RunObservationReason,
    ) -> Result<RunObservationUpdate, OptimizationObservationError> {
        self.snapshot = read_snapshot(&self.state_dir, &self.run_id)?;
        self.pending = None;
        Ok(RunObservationUpdate {
            reason,
            snapshot: self.snapshot.clone(),
        })
    }
}

fn read_snapshot(
    state_dir: &Path,
    run_id: &str,
) -> Result<OptimizationRunSnapshot, OptimizationObservationError> {
    let directory = state_dir.join("optimize").join(run_id);
    for _ in 0..4 {
        let before = read_bytes(&directory.join("current.json"))?;
        let run: OptimizationRunRecord = read_json(&directory.join("run.json"))?;
        let current: serde_json::Value = serde_json::from_slice(&before).map_err(store_error)?;
        let current_cycle = current
            .get("cycle")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                OptimizationObservationError::Store("current.json lacks cycle".into())
            })?;
        let mut cycles = Vec::new();
        let entries = fs::read_dir(directory.join("cycles")).map_err(store_error)?;
        for entry in entries {
            let path = entry.map_err(store_error)?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let cycle: OptimizationCycleRecord = read_json(&path)?;
                if cycle.run_id != run_id || cycle.cycle > current_cycle {
                    return Err(OptimizationObservationError::ScopeMismatch);
                }
                cycles.push(cycle);
            }
        }
        cycles.sort_by_key(|cycle| cycle.cycle);
        let terminal = current
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|phase| matches!(phase, "finished" | "failed"));
        let outcome = if terminal {
            match fs::read(directory.join("outcome.json")) {
                Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(store_error)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(store_error(error)),
            }
        } else {
            None
        };
        let after = read_bytes(&directory.join("current.json"))?;
        if before == after {
            if run.run_id != run_id {
                return Err(OptimizationObservationError::ScopeMismatch);
            }
            return Ok(OptimizationRunSnapshot {
                run,
                current,
                cycles,
                outcome,
            });
        }
        std::thread::yield_now();
    }
    Err(OptimizationObservationError::SnapshotBusy)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, OptimizationObservationError> {
    fs::read(path).map_err(store_error)
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, OptimizationObservationError> {
    serde_json::from_slice(&read_bytes(path)?).map_err(store_error)
}

fn store_error(error: impl std::fmt::Display) -> OptimizationObservationError {
    OptimizationObservationError::Store(error.to_string())
}

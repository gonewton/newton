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
        if run_id.trim().is_empty()
            || Path::new(run_id).components().count() != 1
            || !matches!(
                Path::new(run_id).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
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
        let cycles = read_cycle_history(&directory, run_id);
        let terminal = current
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|phase| matches!(phase, "finished" | "failed"));
        let outcome: Option<OptimizationOutcome> = if terminal {
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
            let cycles = cycles?;
            if run.run_id != run_id
                || current.get("run_id").and_then(serde_json::Value::as_str) != Some(run_id)
                || cycles.iter().any(|cycle| cycle.cycle > current_cycle)
                || outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.run_id != run_id)
            {
                return Err(OptimizationObservationError::ScopeMismatch);
            }
            if run.schema_version != 1
                || current
                    .get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                    != Some(1)
            {
                return Err(store_error("unsupported optimization history schema"));
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

/// Read and validate immutable Cycle history in numeric order. Unknown schema
/// versions, partial files, gaps, duplicate identities and foreign runs fail closed.
pub fn read_cycle_history(
    directory: &Path,
    run_id: &str,
) -> Result<Vec<OptimizationCycleRecord>, OptimizationObservationError> {
    let mut cycles = Vec::new();
    for entry in fs::read_dir(directory.join("cycles")).map_err(store_error)? {
        let path = entry.map_err(store_error)?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let cycle: OptimizationCycleRecord = read_json(&path)?;
            if cycle.schema_version != 1 {
                return Err(store_error("unsupported Cycle history schema"));
            }
            if cycle.run_id != run_id
                || path.file_name().and_then(|name| name.to_str())
                    != Some(&format!("{:04}.json", cycle.cycle))
            {
                return Err(store_error(
                    "Cycle filename or run identity conflicts with history",
                ));
            }
            cycles.push(cycle);
        }
    }
    cycles.sort_by_key(|cycle| cycle.cycle);
    if cycles
        .iter()
        .enumerate()
        .any(|(index, cycle)| cycle.cycle != index as u64 + 1)
    {
        return Err(store_error("Cycle history is non-contiguous"));
    }
    Ok(cycles)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, OptimizationObservationError> {
    fs::read(path).map_err(store_error)
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, OptimizationObservationError> {
    serde_json::from_slice(&read_bytes(path)?)
        .map_err(|error| store_error(format!("{}: {error}", path.display())))
}

fn store_error(error: impl std::fmt::Display) -> OptimizationObservationError {
    OptimizationObservationError::Store(error.to_string())
}

#[cfg(test)]
mod history_tests {
    use super::*;

    fn record(cycle: u64) -> OptimizationCycleRecord {
        OptimizationCycleRecord {
            schema_version: 1,
            run_id: "run".into(),
            cycle,
            requirements_revision: 1,
            started_at: "start".into(),
            completed_at: "end".into(),
            baseline_evaluations: vec![],
            proposal: None,
            execution_id: None,
            execution: None,
            candidate_evaluations: vec![],
            decision: None,
            retained_result: None,
            status: CycleStatus::NoActionableWork,
            diagnostics: vec![],
        }
    }

    fn write(root: &Path, value: &OptimizationCycleRecord) {
        fs::write(
            root.join("cycles").join(format!("{:04}.json", value.cycle)),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn cycle_reader_rejects_unknown_schemas_foreign_runs_and_missing_records() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("cycles")).unwrap();
        let mut value = record(1);
        value.schema_version = 2;
        write(root.path(), &value);
        assert!(read_cycle_history(root.path(), "run").is_err());
        value.schema_version = 1;
        value.run_id = "foreign".into();
        write(root.path(), &value);
        assert!(read_cycle_history(root.path(), "run").is_err());
        write(root.path(), &record(1));
        write(root.path(), &record(3));
        assert!(read_cycle_history(root.path(), "run").is_err());
        write(root.path(), &record(2));
        assert_eq!(read_cycle_history(root.path(), "run").unwrap().len(), 3);
    }

    #[test]
    fn cycle_reader_orders_numerically_after_four_digit_names() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("cycles")).unwrap();
        for cycle in 1..=10_000 {
            write(root.path(), &record(cycle));
        }
        let history = read_cycle_history(root.path(), "run").unwrap();
        assert_eq!(history[9999].cycle, 10_000);
    }
}

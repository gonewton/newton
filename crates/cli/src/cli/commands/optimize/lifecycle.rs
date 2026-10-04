//! Authoritative per-run JSON lifecycle. Completed Cycle files are immutable.

use super::ownership::RunClaim;
use anyhow::{Context, Result};
use newton_types::{optimization::*, BroadcastEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::sync::broadcast;

pub(super) const HISTORY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Ready,
    Working,
    Evaluating,
    Evaluated,
    CycleComplete,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ActiveDispatch {
    pub id: String,
    pub role: String,
    pub started_at: String,
}

/// Small mutable recovery checkpoint. Historical evidence lives in Cycle files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub schema_version: u32,
    pub run_id: String,
    pub cycle: u64,
    pub phase: Phase,
    #[serde(default)]
    pub requires_reconciliation: bool,
    pub binding: Value,
    pub definition_root: PathBuf,
    #[serde(default)]
    pub definition_snapshot: Option<super::snapshot::Manifest>,
    #[serde(default)]
    pub cycle_started_at: Option<String>,
    #[serde(default)]
    pub active_dispatch: Option<ActiveDispatch>,
    #[serde(default)]
    pub candidate: Option<Value>,
    #[serde(default)]
    pub proposal: Option<ProposalOutput>,
    #[serde(default)]
    pub execution_output: Option<ExecutionOutput>,
    #[serde(default)]
    pub baseline_evaluations: Vec<EvaluationOutput>,
    #[serde(default)]
    pub candidate_evaluations: Vec<EvaluationOutput>,
    #[serde(default)]
    pub decision: Option<CandidateDecision>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub evidence: Option<Value>,
    /// Result that qualifies under the active requirements revision.
    #[serde(default)]
    pub accepted: Option<Value>,
    /// Best artifact retained even while active qualification is unavailable.
    #[serde(default)]
    pub retained: Option<Value>,
    #[serde(default)]
    pub accepted_history: Vec<Value>,
    #[serde(default)]
    pub revisions: Vec<RequirementsRevision>,
    #[serde(default)]
    pub outcome: Option<Value>,
    #[serde(default)]
    pub consecutive_no_improvement: u64,
    #[serde(default)]
    pub threshold_baselines: std::collections::BTreeMap<String, f64>,
    #[serde(default)]
    pub projection_prepared: bool,
    #[serde(default)]
    pub projection_configuration: Option<newton_projections::RunProjectionConfiguration>,
    #[serde(default)]
    pub projection_report: Option<newton_projections::RunProjectionReport>,
    pub work_count: u64,
    pub evaluation_count: u64,
    pub started_at: String,
}

pub(super) struct Lifecycle {
    pub journal: Journal,
    pub events: broadcast::Sender<BroadcastEvent>,
    directory: PathBuf,
    path: PathBuf,
    claim: Option<RunClaim>,
}

impl Lifecycle {
    pub fn start(
        events: broadcast::Sender<BroadcastEvent>,
        state_dir: &Path,
        context_root: &Path,
        run_id: &str,
        binding: Value,
    ) -> Result<Self> {
        let claim = RunClaim::acquire(&context_root.join(".newton/optimize/claim"), run_id)?;
        let directory = state_dir.join("optimize").join(run_id);
        let path = directory.join("current.json");
        if path.exists() || directory.join("run.json").exists() {
            anyhow::bail!("Optimize Run {run_id} already exists; use explicit resume");
        }
        fs::create_dir_all(directory.join("cycles"))?;
        let started_at = chrono::Utc::now().to_rfc3339();
        let run = OptimizationRunRecord {
            schema_version: HISTORY_SCHEMA_VERSION,
            run_id: run_id.into(),
            created_at: started_at.clone(),
            binding: binding.clone(),
        };
        write_new_json(&directory.join("run.json"), &run)?;
        let journal = Journal {
            schema_version: HISTORY_SCHEMA_VERSION,
            run_id: run_id.into(),
            cycle: 0,
            phase: Phase::Ready,
            requires_reconciliation: false,
            binding,
            definition_root: PathBuf::new(),
            definition_snapshot: None,
            cycle_started_at: None,
            active_dispatch: None,
            candidate: None,
            proposal: None,
            execution_output: None,
            baseline_evaluations: Vec::new(),
            candidate_evaluations: Vec::new(),
            decision: None,
            execution_id: None,
            evidence: None,
            accepted: None,
            retained: None,
            accepted_history: Vec::new(),
            revisions: Vec::new(),
            outcome: None,
            consecutive_no_improvement: 0,
            threshold_baselines: Default::default(),
            projection_prepared: false,
            projection_configuration: None,
            projection_report: None,
            work_count: 0,
            evaluation_count: 0,
            started_at,
        };
        let lifecycle = Self {
            journal,
            events,
            directory,
            path,
            claim: Some(claim),
        };
        lifecycle.save()?;
        lifecycle.publish(None);
        Ok(lifecycle)
    }

    pub fn resume(
        journal: Journal,
        events: broadcast::Sender<BroadcastEvent>,
        state_dir: &Path,
        context_root: &Path,
    ) -> Result<Self> {
        // Serialize before reading or reopening the checkpoint. A competing
        // resume must never overwrite the running owner's newer checkpoint.
        let claim = RunClaim::resume(
            &context_root.join(".newton/optimize/claim"),
            &journal.run_id,
        )?;
        let directory = state_dir.join("optimize").join(&journal.run_id);
        let expected_run_id = journal.run_id;
        let mut journal: Journal = read_json(&directory.join("current.json"))?;
        anyhow::ensure!(
            journal.run_id == expected_run_id,
            "recovery checkpoint identity changed while acquiring ownership"
        );
        if journal.schema_version != HISTORY_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported optimization checkpoint schema {}",
                journal.schema_version
            );
        }
        if journal.requires_reconciliation || journal.active_dispatch.is_some() {
            anyhow::bail!("Optimize Run {} stopped during {:?}; external effects may have occurred. Reconcile the recorded dispatch before resuming; it will not be replayed", journal.run_id, journal.phase);
        }
        if journal.phase == Phase::Finished
            && journal
                .outcome
                .as_ref()
                .and_then(|value| value.get("stop_reason"))
                == Some(&serde_json::json!("cycle_complete"))
        {
            journal.phase = Phase::CycleComplete;
            journal.outcome = None;
        }
        let history = read_cycles(&directory, &journal.run_id);
        let history = match history {
            Ok(history) => history,
            Err(error) => {
                journal.requires_reconciliation = true;
                newton_core::fs_util::atomic_write(
                    &directory.join("current.json"),
                    &serde_json::to_vec_pretty(&journal)?,
                )?;
                return Err(error);
            }
        };
        let committed = history.last().map_or(0, |record| record.cycle);
        anyhow::ensure!(
            committed <= journal.cycle
                && (committed == journal.cycle
                    || (journal.phase != Phase::CycleComplete && committed + 1 == journal.cycle)),
            "Cycle history is missing a committed Cycle or is ahead of the checkpoint"
        );
        let recovered_cycle = recover_published_cycle(&directory, &mut journal)?;
        if !matches!(
            journal.phase,
            Phase::Ready | Phase::CycleComplete | Phase::Evaluated
        ) {
            anyhow::bail!("Optimize Run {} stopped during {:?}; external effects may have occurred. Reconcile the recorded dispatch before resuming; it will not be replayed", journal.run_id, journal.phase);
        }
        let run: OptimizationRunRecord = read_json(&directory.join("run.json"))?;
        if run.run_id != journal.run_id || run.schema_version != HISTORY_SCHEMA_VERSION {
            anyhow::bail!("run metadata and recovery checkpoint identity disagree");
        }
        let lifecycle = Self {
            journal,
            events,
            path: directory.join("current.json"),
            directory,
            claim: Some(claim),
        };
        if recovered_cycle || lifecycle.journal.phase == Phase::CycleComplete {
            lifecycle.save()?;
            super::remove_if_present(&lifecycle.directory.join("outcome.json"))?;
            super::remove_if_present(&lifecycle.directory.join("report.json"))?;
        }
        Ok(lifecycle)
    }

    pub fn save(&self) -> Result<()> {
        newton_core::fs_util::atomic_write(&self.path, &serde_json::to_vec_pretty(&self.journal)?)
            .with_context(|| format!("persist optimization checkpoint {}", self.path.display()))
    }

    pub fn phase(&mut self, phase: Phase) -> Result<()> {
        self.journal.phase = phase;
        self.save()?;
        self.publish(Some(self.journal.cycle as i64));
        Ok(())
    }

    pub fn complete_cycle(&mut self, status: CycleStatus, diagnostics: Vec<String>) -> Result<()> {
        let cycle = self.journal.cycle;
        anyhow::ensure!(cycle > 0, "cannot publish Cycle zero");
        let record = OptimizationCycleRecord {
            schema_version: HISTORY_SCHEMA_VERSION,
            run_id: self.journal.run_id.clone(),
            cycle,
            requirements_revision: serde_json::from_value::<BoundOptimizationDefinition>(
                self.journal.binding.clone(),
            )?
            .requirements
            .revision,
            started_at: self
                .journal
                .cycle_started_at
                .clone()
                .context("completed Cycle has no start time")?,
            completed_at: chrono::Utc::now().to_rfc3339(),
            baseline_evaluations: self.journal.baseline_evaluations.clone(),
            proposal: self.journal.proposal.clone(),
            execution_id: self.journal.execution_id.clone(),
            execution: self.journal.execution_output.clone(),
            candidate_evaluations: self.journal.candidate_evaluations.clone(),
            decision: self.journal.decision.clone(),
            retained_result: self
                .journal
                .retained
                .clone()
                .map(serde_json::from_value)
                .transpose()?,
            status,
            diagnostics,
        };
        let cycle_path = self
            .directory
            .join("cycles")
            .join(format!("{cycle:04}.json"));
        if cycle_path.exists() {
            let existing: OptimizationCycleRecord = read_json(&cycle_path)?;
            if existing != record {
                anyhow::bail!("published Cycle {cycle} conflicts with recovery state; reconciliation required");
            }
        } else {
            write_new_json(&cycle_path, &record)?;
        }
        // The Cycle file is the commit point. Advance the checkpoint only after it exists.
        self.journal.phase = Phase::CycleComplete;
        self.journal.active_dispatch = None;
        self.save()?;
        self.publish(Some(cycle as i64));
        Ok(())
    }

    pub fn finish(&mut self, outcome: Value, safe_to_release: bool) -> Result<()> {
        if !safe_to_release {
            self.journal.requires_reconciliation = true;
        }
        self.journal.phase = if safe_to_release {
            Phase::Finished
        } else {
            Phase::Failed
        };
        self.journal.outcome = Some(outcome.clone());
        newton_core::fs_util::atomic_write(
            &self.directory.join("outcome.json"),
            &serde_json::to_vec_pretty(&outcome)?,
        )?;
        // The outcome and recovery state are authoritative. Persist them before
        // deriving the convenience report so corrupt historical input cannot
        // erase the fact that this run now needs reconciliation.
        self.save()?;
        let typed_outcome: OptimizationOutcome = serde_json::from_value(outcome)?;
        let cycles = match read_cycles(&self.directory, &self.journal.run_id) {
            Ok(cycles) => cycles,
            Err(error) => {
                self.journal.phase = Phase::Failed;
                self.journal.requires_reconciliation = true;
                self.save()?;
                self.publish(None);
                return Err(error);
            }
        };
        let report = OptimizationReport {
            schema_version: HISTORY_SCHEMA_VERSION,
            run_id: self.journal.run_id.clone(),
            before: cycles
                .first()
                .map(|cycle| cycle.baseline_evaluations.clone())
                .unwrap_or_default(),
            after: typed_outcome.retained_result.clone(),
            cycles,
            outcome: typed_outcome,
        };
        newton_core::fs_util::atomic_write(
            &self.directory.join("report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        self.publish(None);
        if safe_to_release {
            if let Some(claim) = self.claim.take() {
                claim.release()?;
            }
        }
        Ok(())
    }

    fn publish(&self, cycle: Option<i64>) {
        let _ = self.events.send(BroadcastEvent::OptimizeRunUpdate {
            run_id: self.journal.run_id.clone(),
            cycle,
        });
    }
}

fn recover_published_cycle(directory: &Path, journal: &mut Journal) -> Result<bool> {
    let path = directory
        .join("cycles")
        .join(format!("{:04}.json", journal.cycle));
    if journal.cycle > 0 && path.exists() && journal.phase != Phase::CycleComplete {
        let record: OptimizationCycleRecord = read_json(&path)?;
        anyhow::ensure!(
            record.run_id == journal.run_id && record.cycle == journal.cycle,
            "published Cycle identity conflicts with recovery checkpoint"
        );
        anyhow::ensure!(
            record.decision == journal.decision
                && record.proposal == journal.proposal
                && record.candidate_evaluations == journal.candidate_evaluations
                && record.baseline_evaluations == journal.baseline_evaluations,
            "published Cycle evidence conflicts with recovery checkpoint"
        );
        // A completed Cycle is committed by the immutable file. A crash before
        // current.json advances is safe to recover without replaying execution.
        journal.phase = Phase::CycleComplete;
        journal.active_dispatch = None;
        return Ok(true);
    }
    Ok(false)
}

fn write_new_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("JSON artifact has no parent")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?).with_context(|| format!("read {}", path.display()))
}

fn read_cycles(directory: &Path, run_id: &str) -> Result<Vec<OptimizationCycleRecord>> {
    Ok(newton_core::optimization::read_cycle_history(
        directory, run_id,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_publication_never_overwrites_existing_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.json");
        write_new_json(&path, &serde_json::json!({"original": true})).unwrap();
        let original = fs::read(&path).unwrap();
        assert!(write_new_json(&path, &serde_json::json!({"replacement": true})).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn lifecycle_starts_without_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let (events, mut receiver) = broadcast::channel(16);
        let lifecycle = Lifecycle::start(
            events,
            dir.path(),
            dir.path(),
            "run-one",
            serde_json::json!({}),
        )
        .unwrap();
        assert!(
            matches!(receiver.try_recv().unwrap(), BroadcastEvent::OptimizeRunUpdate { run_id, cycle: None } if run_id == "run-one")
        );
        assert!(dir.path().join("optimize/run-one/run.json").is_file());
        assert!(dir.path().join("optimize/run-one/current.json").is_file());
        assert!(!dir.path().join("backend.sqlite").exists());
        drop(lifecycle);
    }
}

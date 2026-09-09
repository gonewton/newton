//! Durable native Run/Cycle lifecycle; workflow effects are never replayed blindly.

use super::ownership::RunClaim;
use anyhow::{anyhow, Context, Result};
use newton_types::{
    BackendStore, BroadcastEvent, CreateOptimizeCycleBody, CreateOptimizeRunBody,
    PatchOptimizeRunBody,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Ready,
    Working,
    Evaluating,
    Evaluated,
    Promoting,
    CycleComplete,
    Finished,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_lifecycle_persists_and_publishes_real_run_cycle_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn BackendStore> = Arc::new(
            newton_backend::SqliteBackendStore::new(&format!(
                "sqlite:{}?mode=rwc",
                dir.path().join("backend.sqlite").display()
            ))
            .await
            .unwrap(),
        );
        let (events, mut receiver) = broadcast::channel(16);
        let mut lifecycle = Lifecycle::start(
            store.clone(),
            events,
            dir.path(),
            dir.path(),
            "run-one",
            "context",
            2,
            vec!["fixture".into()],
            serde_json::json!({}),
        )
        .await
        .unwrap();
        assert!(
            matches!(receiver.recv().await.unwrap(), BroadcastEvent::OptimizeRunUpdate { run_id, cycle: None } if run_id == "run-one")
        );
        lifecycle.journal.cycle = 1;
        lifecycle.journal.evidence = Some(serde_json::json!({"verified": true}));
        lifecycle
            .complete_cycle("accepted", Some("execution-one".into()))
            .await
            .unwrap();
        assert!(
            matches!(receiver.recv().await.unwrap(), BroadcastEvent::OptimizeRunUpdate { run_id, cycle: Some(1) } if run_id == "run-one")
        );
        lifecycle
            .finish(
                "cycle_complete",
                serde_json::json!({"completed": false}),
                true,
            )
            .await
            .unwrap();
        let stored = store.get_optimize_run("run-one").await.unwrap();
        assert_eq!(stored.run.status, "cycle_complete");
        assert_eq!(
            store.list_optimize_cycles("run-one").await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn resume_requires_exclusive_owner_and_a_known_phase() {
        let dir = tempfile::tempdir().unwrap();
        let store: Arc<dyn BackendStore> = Arc::new(
            newton_backend::SqliteBackendStore::new(&format!(
                "sqlite:{}?mode=rwc",
                dir.path().join("backend.sqlite").display()
            ))
            .await
            .unwrap(),
        );
        let (events, _) = broadcast::channel(16);
        let lifecycle = Lifecycle::start(
            store.clone(),
            events.clone(),
            dir.path(),
            dir.path(),
            "run-one",
            "context",
            2,
            vec![],
            serde_json::json!({}),
        )
        .await
        .unwrap();
        let ready = lifecycle.journal.clone();
        assert!(Lifecycle::resume(
            ready.clone(),
            store.clone(),
            events.clone(),
            dir.path(),
            dir.path()
        )
        .await
        .is_err());
        drop(lifecycle);
        let mut resumed =
            Lifecycle::resume(ready, store.clone(), events.clone(), dir.path(), dir.path())
                .await
                .unwrap();
        resumed.phase(Phase::Promoting).await.unwrap();
        let uncertain = resumed.journal.clone();
        drop(resumed);
        let error = Lifecycle::resume(uncertain, store, events, dir.path(), dir.path())
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("Reconcile"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Journal {
    pub run_id: String,
    pub cycle: u64,
    pub phase: Phase,
    pub binding: Value,
    pub definition_root: PathBuf,
    pub candidate: Option<Value>,
    pub plan_id: Option<String>,
    pub execution_id: Option<String>,
    pub evidence: Option<Value>,
    pub accepted: Option<Value>,
    #[serde(default)]
    pub accepted_history: Vec<Value>,
    #[serde(default)]
    pub revisions: Vec<newton_types::optimization::RequirementsRevision>,
    pub outcome: Option<Value>,
    pub work_count: u64,
    pub evaluation_count: u64,
    pub started_at: String,
}

pub(super) struct Lifecycle {
    pub journal: Journal,
    pub store: Arc<dyn BackendStore>,
    pub events: broadcast::Sender<BroadcastEvent>,
    path: PathBuf,
    claim: Option<RunClaim>,
}

impl Lifecycle {
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        store: Arc<dyn BackendStore>,
        events: broadcast::Sender<BroadcastEvent>,
        state_dir: &Path,
        context_root: &Path,
        run_id: &str,
        project_id: &str,
        max_cycles: u64,
        graders: Vec<String>,
        binding: Value,
    ) -> Result<Self> {
        // Context-wide ownership prevents two native consumers from producing or
        // promoting changes to the same repository, even with different run IDs.
        let claim = RunClaim::acquire(&context_root.join(".newton/optimize/claim"), run_id)?;
        let path = state_dir.join("optimize").join(run_id).join("journal.json");
        if path.exists() {
            anyhow::bail!("Optimize Run {run_id} already exists; use explicit resume");
        }
        let journal = Journal {
            run_id: run_id.into(),
            cycle: 0,
            phase: Phase::Ready,
            binding,
            definition_root: PathBuf::new(),
            candidate: None,
            plan_id: None,
            execution_id: None,
            evidence: None,
            accepted: None,
            accepted_history: Vec::new(),
            revisions: Vec::new(),
            outcome: None,
            work_count: 0,
            evaluation_count: 0,
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        let lifecycle = Self {
            journal,
            store,
            events,
            path,
            claim: Some(claim),
        };
        lifecycle.save()?;
        lifecycle
            .store
            .create_optimize_run(CreateOptimizeRunBody {
                id: run_id.into(),
                project_id: project_id.into(),
                scope: "repo".into(),
                scope_id: project_id.into(),
                max_cycles: i64::try_from(max_cycles)?,
                graders,
            })
            .await
            .map_err(|e| anyhow!("create Optimize Run: {}", e.message))?;
        lifecycle.publish(None);
        Ok(lifecycle)
    }

    pub async fn resume(
        journal: Journal,
        store: Arc<dyn BackendStore>,
        events: broadcast::Sender<BroadcastEvent>,
        state_dir: &Path,
        context_root: &Path,
    ) -> Result<Self> {
        if !matches!(
            journal.phase,
            Phase::Ready | Phase::CycleComplete | Phase::Evaluated
        ) {
            anyhow::bail!("Optimize Run {} stopped during {:?}; external effects may have occurred. Reconcile the recorded work before resuming; it will not be replayed", journal.run_id, journal.phase);
        }
        let claim = RunClaim::resume(
            &context_root.join(".newton/optimize/claim"),
            &journal.run_id,
        )?;
        store.get_optimize_run(&journal.run_id).await.map_err(|e| {
            anyhow!(
                "resume requires matching durable Optimize Run: {}",
                e.message
            )
        })?;
        let path = state_dir
            .join("optimize")
            .join(&journal.run_id)
            .join("journal.json");
        Ok(Self {
            journal,
            store,
            events,
            path,
            claim: Some(claim),
        })
    }

    pub fn save(&self) -> Result<()> {
        newton_core::fs_util::atomic_write(&self.path, &serde_json::to_vec_pretty(&self.journal)?)
            .with_context(|| format!("persist optimization journal {}", self.path.display()))
    }

    pub async fn phase(&mut self, phase: Phase) -> Result<()> {
        self.journal.phase = phase;
        self.save()?;
        self.store
            .patch_optimize_run(
                &self.journal.run_id,
                PatchOptimizeRunBody {
                    cycle: Some(i64::try_from(self.journal.cycle)?),
                    outcome_reason: Some(serde_json::to_value(&self.journal)?),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| anyhow!("persist Optimize Run phase: {}", e.message))?;
        self.publish(Some(self.journal.cycle as i64));
        Ok(())
    }

    pub async fn complete_cycle(
        &mut self,
        decision: &str,
        execution_id: Option<String>,
    ) -> Result<()> {
        let cycle = self.journal.cycle;
        let existing = self
            .store
            .list_optimize_cycles(&self.journal.run_id)
            .await
            .map_err(|e| anyhow!("read durable Optimize Cycles: {}", e.message))?;
        if let Some(existing) = existing.iter().find(|c| c.cycle == cycle as i64) {
            if existing.decision != decision
                || existing.grades != self.journal.evidence.clone().unwrap_or(Value::Null)
            {
                anyhow::bail!("persisted Optimize Cycle conflicts with resumed decision; reconciliation required");
            }
            return self.phase(Phase::CycleComplete).await;
        }
        self.store
            .create_optimize_cycle(CreateOptimizeCycleBody {
                id: format!("{}-{cycle}", self.journal.run_id),
                run_id: self.journal.run_id.clone(),
                cycle: i64::try_from(cycle)?,
                grades: self.journal.evidence.clone().unwrap_or(Value::Null),
                grade_min: None,
                decision: decision.into(),
                change_request_id: None,
                plan_id: self.journal.plan_id.clone(),
                execution_id: execution_id.or_else(|| self.journal.execution_id.clone()),
                develop_status: Some(decision.into()),
                open_findings: 0,
                resolved_this_cycle: 0,
            })
            .await
            .map_err(|e| anyhow!("persist Optimize Cycle: {}", e.message))?;
        self.phase(Phase::CycleComplete).await
    }

    pub async fn finish(
        &mut self,
        status: &str,
        outcome: Value,
        safe_to_release: bool,
    ) -> Result<()> {
        if self.journal.cycle > 0 && self.journal.phase != Phase::CycleComplete {
            self.complete_cycle(status, None).await?;
        }
        self.journal.phase = if status == "failed" {
            Phase::Failed
        } else {
            Phase::Finished
        };
        self.journal.outcome = Some(outcome.clone());
        self.save()?;
        self.store
            .patch_optimize_run(
                &self.journal.run_id,
                PatchOptimizeRunBody {
                    status: Some(status.into()),
                    cycle: Some(i64::try_from(self.journal.cycle)?),
                    outcome_reason: Some(outcome),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| anyhow!("persist Optimize Run outcome: {}", e.message))?;
        self.publish(None);
        if safe_to_release {
            if let Some(claim) = self.claim.take() {
                claim.release()?;
            }
        }
        Ok(())
    }

    fn publish(&self, cycle: Option<i64>) {
        // No listeners is normal for local CLI operation; the same publisher can
        // be supplied by an embedding observer instead of inventing HTTP writes.
        let _ = self.events.send(BroadcastEvent::OptimizeRunUpdate {
            run_id: self.journal.run_id.clone(),
            cycle,
        });
    }
}

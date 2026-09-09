//! Software-only work identity and recovery. The generic strategy has no entity spine.

use anyhow::{anyhow, Context, Result};
use newton_types::{BackendStore, FindingStatus, PatchFindingBody, PatchPlanBody};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Ledger {
    pub work: BTreeMap<String, LogicalWork>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LogicalWork {
    pub finding_ids: Vec<String>,
    pub plan_ids: Vec<String>,
    pub failures: u64,
    pub quarantined: bool,
    pub completed: bool,
    pub last_error: Option<String>,
    pub recovery_evidence: Vec<String>,
    #[serde(default)]
    pub failed_plan_ids: BTreeSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReconciledFailure {
    pub change_request_id: String,
    pub plan_id: String,
    pub reason: String,
    pub reconciled: bool,
    pub evidence: Vec<String>,
}

impl Ledger {
    pub fn blocked(&self) -> Vec<String> {
        self.work
            .iter()
            .filter(|(_, work)| work.quarantined)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Validate exact returned identities, never the latest repository record.
    pub async fn prepare(
        &mut self,
        store: &dyn BackendStore,
        cr_id: &str,
        plan_id: &str,
    ) -> Result<()> {
        let cr = store
            .get_change_request(cr_id)
            .await
            .map_err(|e| anyhow!("load selected Change Request {cr_id}: {}", e.message))?;
        let plan = store
            .get_plan(plan_id)
            .await
            .map_err(|e| anyhow!("load selected Plan {plan_id}: {}", e.message))?
            .plan;
        if plan.linked_change_request_id.as_deref() != Some(cr_id) {
            anyhow::bail!("selected Plan does not belong to the current Change Request");
        }
        if plan.status != "ready" {
            anyhow::bail!(
                "selected Plan must be ready, not {}; completed attempts are never replayed",
                plan.status
            );
        }
        let mut finding_ids = cr.finding_ids;
        finding_ids.sort();
        finding_ids.dedup();
        if finding_ids.is_empty() {
            anyhow::bail!("software-improvement Change Request must reference reconciled Findings");
        }
        for id in &finding_ids {
            let finding = store
                .get_finding(id)
                .await
                .map_err(|e| anyhow!("load selected Finding {id}: {}", e.message))?;
            if matches!(
                finding.status,
                FindingStatus::Blocked
                    | FindingStatus::Deferred
                    | FindingStatus::Rejected
                    | FindingStatus::Resolved
            ) {
                anyhow::bail!(
                    "planner selected ineligible Finding {id}: {}",
                    finding.status
                );
            }
        }
        let work = self.work.entry(cr_id.into()).or_default();
        if work.quarantined || work.completed {
            anyhow::bail!("planner selected quarantined or completed Change Request {cr_id}");
        }
        if !work.finding_ids.is_empty() && work.finding_ids != finding_ids {
            anyhow::bail!("Change Request Findings changed during logical retry accounting");
        }
        work.finding_ids = finding_ids;
        if work.plan_ids.iter().any(|id| id == plan_id) {
            anyhow::bail!(
                "Plan {plan_id} was already dispatched; completed attempts are never replayed"
            );
        }
        work.plan_ids.push(plan_id.into());
        Ok(())
    }

    pub fn fail(
        &mut self,
        cr_id: &str,
        plan_id: &str,
        failure: ReconciledFailure,
        limit: u64,
    ) -> Result<bool> {
        if failure.change_request_id != cr_id || failure.plan_id != plan_id {
            anyhow::bail!("develop failure belongs to another Change Request or Plan");
        }
        if !failure.reconciled
            || failure.reason.trim().is_empty()
            || failure.evidence.is_empty()
            || failure.evidence.iter().any(|e| e.trim().is_empty())
        {
            anyhow::bail!("develop failure has no explicit reconciled recovery evidence; intervention required");
        }
        self.record_failure(cr_id, plan_id, failure.reason, failure.evidence, limit)
    }

    /// A completed, validated evaluation rejected this candidate. This is not
    /// an execution error or a claim that arbitrary external effects rolled back.
    pub fn reject_candidate(
        &mut self,
        cr_id: &str,
        plan_id: &str,
        evaluation_id: &str,
        limit: u64,
    ) -> Result<bool> {
        self.record_failure(
            cr_id,
            plan_id,
            format!("candidate rejected by evaluation {evaluation_id}"),
            vec![format!("evaluation:{evaluation_id}")],
            limit,
        )
    }

    fn record_failure(
        &mut self,
        cr_id: &str,
        plan_id: &str,
        reason: String,
        evidence: Vec<String>,
        limit: u64,
    ) -> Result<bool> {
        let work = self
            .work
            .get_mut(cr_id)
            .context("selected logical work missing")?;
        if !work.failed_plan_ids.insert(plan_id.to_owned()) {
            return Ok(work.quarantined);
        }
        work.failures = work
            .failures
            .checked_add(1)
            .context("logical retry counter overflow")?;
        work.last_error = Some(reason);
        work.recovery_evidence = evidence;
        work.quarantined = work.failures >= limit;
        Ok(work.quarantined)
    }

    /// Called after write-ahead journal persistence. Any store error fails the run.
    pub async fn persist_failure(
        &self,
        store: &dyn BackendStore,
        cr_id: &str,
        plan_id: &str,
        execution_id: Option<String>,
    ) -> Result<()> {
        let work = self
            .work
            .get(cr_id)
            .context("selected logical work missing")?;
        store
            .patch_plan(
                plan_id,
                PatchPlanBody {
                    status: Some("failed".into()),
                    attempts: Some(i64::try_from(work.failures)?),
                    last_error: work.last_error.clone(),
                    execution_id,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| anyhow!("persist failed Plan: {}", e.message))?;
        if work.quarantined {
            for id in &work.finding_ids {
                store
                    .patch_finding(
                        id,
                        PatchFindingBody {
                            status: Some(FindingStatus::Blocked),
                            blocked_by_plan_id: Some(plan_id.into()),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| anyhow!("quarantine Finding {id}: {}", e.message))?;
            }
        }
        Ok(())
    }
}

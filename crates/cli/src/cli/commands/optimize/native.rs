//! Native, definition-bound workflow coordinator.

use super::envelopes::{DevelopOutput, GradeOutput, PlanOutput};
use super::{
    lifecycle::{Journal, Lifecycle, Phase},
    workflow::WorkflowExecutor,
};
use anyhow::{anyhow, Context, Result};
use newton_types::{optimization::*, BackendStore, BroadcastEvent};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tokio::sync::broadcast;

pub(super) struct NativeDriver {
    binding: BoundOptimizationDefinition,
    definition_root: PathBuf,
    runtime: super::snapshot::Runtime,
    executor: WorkflowExecutor,
    lifecycle: Lifecycle,
    started: Instant,
    previous_elapsed: u64,
}

impl NativeDriver {
    pub async fn start(
        binding: BoundOptimizationDefinition,
        definition_root: PathBuf,
        runtime: super::snapshot::Runtime,
        definition_snapshot: super::snapshot::Manifest,
        state_dir: PathBuf,
        events: broadcast::Sender<BroadcastEvent>,
    ) -> Result<Self> {
        definition_snapshot.verify(&binding, &definition_root)?;
        let workspace = PathBuf::from(&binding.context.root).canonicalize()?;
        for role in ["grade", "plan", "develop"] {
            if !binding.definition.workflows.contains_key(role) {
                anyhow::bail!("native software strategy requires workflow role '{role}'");
            }
        }
        if !matches!(
            binding.definition.strategy.as_str(),
            "software-improvement" | "direct-search"
        ) {
            anyhow::bail!(
                "unsupported optimization strategy {}; available: software-improvement, direct-search",
                binding.definition.strategy
            );
        }
        let store: Arc<dyn BackendStore> = Arc::new(
            newton_backend::SqliteBackendStore::new(
                &crate::cli::workspace_paths::state_backend_sqlite_url(&state_dir),
            )
            .await
            .map_err(|e| anyhow!("open optimization store: {}", e.message))?,
        );
        let mut lifecycle = Lifecycle::start(
            store.clone(),
            events,
            &state_dir,
            &workspace,
            &binding.run_id,
            &binding.context.id,
            binding.requirements.requirements.resource_limits.max_cycles,
            binding
                .requirements
                .requirements
                .evaluators
                .keys()
                .cloned()
                .collect(),
            serde_json::to_value(&binding)?,
        )
        .await?;
        lifecycle.journal.definition_root = definition_root.clone();
        lifecycle.journal.definition_snapshot = Some(definition_snapshot);
        lifecycle.save()?;
        let projection_report =
            super::projection::prepare(&mut lifecycle, &workspace, &binding.requirements.authority);
        super::projection::report(&projection_report);
        let driver = Self {
            binding,
            definition_root,
            runtime,
            executor: WorkflowExecutor {
                workspace,
                state_dir,
                store,
            },
            lifecycle,
            started: Instant::now(),
            previous_elapsed: 0,
        };
        Ok(driver)
    }

    pub async fn resume(
        journal: Journal,
        state_dir: PathBuf,
        events: broadcast::Sender<BroadcastEvent>,
    ) -> Result<Self> {
        let runtime = super::snapshot::runtime_from_journal(&journal, &state_dir)?;
        let binding: BoundOptimizationDefinition = serde_json::from_value(journal.binding.clone())?;
        let workspace = PathBuf::from(&binding.context.root).canonicalize()?;
        let definition_root = journal.definition_root.clone();
        let previous_elapsed = (chrono::Utc::now()
            - chrono::DateTime::parse_from_rfc3339(&journal.started_at)?
                .with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0) as u64;
        let store: Arc<dyn BackendStore> = Arc::new(
            newton_backend::SqliteBackendStore::new(
                &crate::cli::workspace_paths::state_backend_sqlite_url(&state_dir),
            )
            .await
            .map_err(|e| anyhow!("open optimization store: {}", e.message))?,
        );
        let lifecycle =
            Lifecycle::resume(journal, store.clone(), events, &state_dir, &workspace).await?;
        Ok(Self {
            binding,
            definition_root,
            runtime,
            executor: WorkflowExecutor {
                workspace,
                state_dir,
                store,
            },
            lifecycle,
            started: Instant::now(),
            previous_elapsed,
        })
    }

    pub async fn run(mut self, once: bool, poll_seconds: u64) -> Result<OptimizationOutcome> {
        // Register before any workflow can publish a dispatch. A lazy ctrl_c
        // future can otherwise miss an interrupt in that publication window.
        #[cfg(unix)]
        let mut interrupts =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .context("install optimization cancellation handler")?;
        let cancelled = async {
            #[cfg(unix)]
            {
                interrupts.recv().await;
                Ok::<(), std::io::Error>(())
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await
            }
        };
        let result = tokio::select! {
            result = self.run_cycles(once, poll_seconds) => result,
            signal = cancelled => {
                signal.context("install cancellation handler")?;
                Err(super::stop::Cancelled.into())
            }
        };
        match result {
            Ok(reason) => {
                let outcome = self.outcome(reason, Vec::new())?;
                let status = match reason {
                    OptimizationStopReason::Completed => "converged",
                    OptimizationStopReason::NoActionableWork => "no_actionable_work",
                    OptimizationStopReason::CycleComplete => "cycle_complete",
                    OptimizationStopReason::Regression => "regressed",
                    OptimizationStopReason::NoProgress => "no_progress",
                    OptimizationStopReason::NeedsIntervention => "stalled_on_blocked",
                    OptimizationStopReason::ResourceLimit => "resource_limit",
                    OptimizationStopReason::OperationalFailure => "failed",
                    OptimizationStopReason::Cancelled => "cancelled",
                };
                self.finish_and_project(status, serde_json::to_value(&outcome)?, true)
                    .await?;
                Ok(outcome)
            }
            Err(error) => {
                if error.is::<super::stop::Cancelled>()
                    || error.is::<super::stop::ResourceExhausted>()
                {
                    let (reason, status, safe) = if let Some(limit) =
                        error.downcast_ref::<super::stop::ResourceExhausted>()
                    {
                        (
                            OptimizationStopReason::ResourceLimit,
                            "resource_limit",
                            !limit.uncertain,
                        )
                    } else {
                        (
                            OptimizationStopReason::Cancelled,
                            "cancelled",
                            matches!(
                                self.lifecycle.journal.phase,
                                Phase::Ready | Phase::CycleComplete
                            ),
                        )
                    };
                    let outcome = self.outcome(reason, vec![error.to_string()])?;
                    self.finish_and_project(status, serde_json::to_value(&outcome)?, safe)
                        .await?;
                    return Ok(outcome);
                }
                let outcome = self.outcome(
                    OptimizationStopReason::OperationalFailure,
                    vec![error.to_string()],
                )?;
                // A failure during external work has an unknown side-effect
                // outcome. Persist the failure but retain ownership for review.
                self.finish_and_project("failed", serde_json::to_value(outcome)?, false)
                    .await?;
                Err(error)
            }
        }
    }

    async fn finish_and_project(
        &mut self,
        status: &str,
        outcome: Value,
        safe_to_release: bool,
    ) -> Result<()> {
        self.lifecycle
            .finish(status, outcome, safe_to_release)
            .await?;
        let report = super::projection::reflect(
            &mut self.lifecycle,
            &self.executor.workspace,
            &self.executor.state_dir,
            &self.binding.requirements.authority,
        )
        .await;
        super::projection::report(&report);
        Ok(())
    }

    fn requirements(&self) -> &OptimizationRequirements {
        &self.binding.requirements.requirements
    }

    pub(super) fn observation_source(
        &self,
    ) -> newton_core::optimization::OptimizeRunObservationSource {
        newton_core::optimization::OptimizeRunObservationSource::new(
            self.lifecycle.store.clone(),
            self.lifecycle.events.clone(),
        )
    }

    fn is_software(&self) -> bool {
        self.binding.definition.strategy == "software-improvement"
    }

    fn failure_limit(&self) -> Result<u64> {
        match self
            .binding
            .requirements
            .parameters
            .get("max_failed_attempts")
        {
            None => Ok(2),
            Some(ParameterValue::Literal { value }) => value
                .as_u64()
                .filter(|n| *n > 0)
                .context("max_failed_attempts must be a positive integer"),
            _ => anyhow::bail!("max_failed_attempts must be a non-secret positive integer"),
        }
    }

    fn remaining_seconds(&self) -> u64 {
        self.requirements()
            .resource_limits
            .elapsed_seconds
            .saturating_sub(self.elapsed())
    }

    fn elapsed(&self) -> u64 {
        self.previous_elapsed
            .saturating_add(self.started.elapsed().as_secs())
    }

    fn triggers(&self) -> Value {
        json!({
            "workspace": self.executor.workspace,
            "run_id": self.binding.run_id,
            "cycle": self.lifecycle.journal.cycle,
            "candidate_id": format!("{}-{}", self.binding.run_id, self.lifecycle.journal.cycle),
            "requirements_revision": self.binding.requirements.revision,
            "requirements": self.requirements(),
            "parameters": self.binding.requirements.parameters,
            "accepted_result": self.lifecycle.journal.accepted,
            "change_request_id": self.lifecycle.journal.change_request_id,
            "blocked_work": self.lifecycle.journal.software_work.blocked(),
            "blocked_work_count": self.lifecycle.journal.software_work.blocked().len(),
            "software_work": self.lifecycle.journal.software_work,
        })
    }

    async fn step(&mut self, role: &str, mut triggers: Value) -> Result<Value> {
        let reference = if role == "grade" {
            super::workflow::grade_reference(self.requirements())?
        } else {
            self.binding
                .definition
                .workflows
                .get(role)
                .with_context(|| format!("missing workflow role {role}"))?
        };
        let path = resolve_reference(&self.definition_root, reference)?;
        let document = self.runtime.workflow(reference)?;
        if role == "grade" {
            triggers["evaluator_workflow"] = json!(reference);
        }
        if role == "grade" {
            if self.lifecycle.journal.evaluation_count
                >= self.requirements().resource_limits.max_evaluations
            {
                return Err(super::stop::ResourceExhausted {
                    uncertain: false,
                    detail: ": evaluation budget exhausted before complete evidence was produced",
                }
                .into());
            }
            self.lifecycle.journal.evaluation_count += 1;
        } else {
            if self.lifecycle.journal.work_count >= self.requirements().resource_limits.max_work {
                return Err(super::stop::ResourceExhausted {
                    uncertain: false,
                    detail: ": work budget exhausted before dispatch",
                }
                .into());
            }
            self.lifecycle.journal.work_count += 1;
        }
        triggers["role"] = json!(role);
        triggers["assets"] = self.runtime.assets();
        triggers["state_dir"] = json!(self.executor.state_dir);
        triggers["remaining_seconds"] = json!(self.remaining_seconds());
        let exchange = self
            .executor
            .state_dir
            .join("optimize")
            .join(&self.binding.run_id)
            .join("workflow-inputs");
        std::fs::create_dir_all(&exchange)?;
        let dispatch = format!(
            "{}-{}-{}-{}",
            self.lifecycle.journal.cycle,
            role,
            self.lifecycle.journal.evaluation_count,
            self.lifecycle.journal.work_count
        );
        let input_file = exchange.join(format!("{dispatch}.json"));
        triggers["input_file"] = json!(input_file);
        triggers["result_file"] = json!(exchange.join(format!("{dispatch}-result.json")));
        newton_core::fs_util::atomic_write(&input_file, &serde_json::to_vec(&triggers)?)?;
        self.lifecycle.save()?;
        let summary = self
            .executor
            .execute(
                &path,
                document,
                triggers,
                self.remaining_seconds(),
                &self.binding.requirements.authority,
            )
            .await?;
        if role == "develop" {
            self.lifecycle.journal.execution_id = Some(summary.execution_id.to_string());
            self.lifecycle.save()?;
        }
        summary.result.with_context(|| format!("{role} workflow must expose an explicit io.result_map; absent output is not no work"))
    }

    async fn run_cycles(
        &mut self,
        once: bool,
        poll_seconds: u64,
    ) -> Result<OptimizationStopReason> {
        if self.lifecycle.journal.phase == Phase::Evaluated {
            let candidate = serde_json::from_value(
                self.lifecycle
                    .journal
                    .candidate
                    .clone()
                    .context("resume evaluated phase requires candidate")?,
            )?;
            let evaluation = serde_json::from_value(
                self.lifecycle
                    .journal
                    .evidence
                    .clone()
                    .context("resume evaluated phase requires evidence")?,
            )?;
            let work_claim = if self.is_software() {
                let cr = self
                    .lifecycle
                    .journal
                    .change_request_id
                    .as_ref()
                    .context("evaluated software work requires its Change Request")?;
                let key = newton_core::workflow::state::compute_sha256_hex(cr.as_bytes());
                Some(super::ownership::RunClaim::resume(
                    &self
                        .executor
                        .state_dir
                        .join("optimize/work-claims")
                        .join(key),
                    &self.binding.run_id,
                )?)
            } else {
                None
            };
            self.accept_evaluated(candidate, evaluation).await?;
            self.finish_software_candidate().await?;
            if let Some(claim) = work_claim {
                claim.release()?;
            }
            if let Some(reason) = self.lifecycle.journal.threshold_history.stop {
                return Ok(reason);
            }
            if once {
                return Ok(OptimizationStopReason::CycleComplete);
            }
        }
        loop {
            if self.remaining_seconds() == 0
                || self.lifecycle.journal.cycle >= self.requirements().resource_limits.max_cycles
                || self.lifecycle.journal.work_count >= self.requirements().resource_limits.max_work
                || self.lifecycle.journal.evaluation_count
                    >= self.requirements().resource_limits.max_evaluations
            {
                return Ok(OptimizationStopReason::ResourceLimit);
            }
            self.lifecycle.journal.cycle += 1;
            self.lifecycle.journal.evidence = None;
            self.lifecycle.journal.plan_id = None;
            self.lifecycle.journal.change_request_id = None;
            self.lifecycle.journal.execution_id = None;
            self.lifecycle.phase(Phase::Evaluating).await?;
            let mut baseline_trigger = self.triggers();
            baseline_trigger["stage"] = json!("baseline");
            baseline_trigger["candidate_id"] = json!(format!(
                "{}-baseline-{}",
                self.binding.run_id, self.lifecycle.journal.cycle
            ));
            if let Some(accepted) = &self.lifecycle.journal.accepted {
                baseline_trigger["candidate_id"] = accepted["candidate"]["id"].clone();
                baseline_trigger["candidate"] = accepted["candidate"].clone();
            }
            let baseline = self.grade(baseline_trigger).await?;
            self.lifecycle.journal.change_request_id = baseline.change_request_id.clone();
            self.lifecycle.journal.open_findings = baseline.open_findings.clone();
            self.validate_cycle(&baseline.evaluation)?;
            let baseline_decision = newton_core::optimization::evaluate_candidate(
                &self.binding.run_id,
                &self.binding.requirements,
                &baseline.candidate,
                &baseline.evaluation,
                None,
            )?;
            refresh_incumbent(
                &mut self.lifecycle.journal,
                &self.binding.requirements,
                baseline_decision.accepted_result,
                &baseline.candidate,
            )?;
            self.lifecycle.journal.evidence = Some(serde_json::to_value(&baseline.evaluation)?);
            if let Some(reason) = self.lifecycle.journal.threshold_history.observe(
                &self.binding.requirements,
                &baseline.evaluation,
                true,
                self.lifecycle.journal.open_findings.as_ref(),
            )? {
                self.lifecycle
                    .complete_cycle("threshold_stop", None)
                    .await?;
                return Ok(reason);
            }
            if self
                .outcome(OptimizationStopReason::ResourceLimit, Vec::new())?
                .completion
                .status
                == CheckStatus::Satisfied
            {
                self.lifecycle.complete_cycle("completed", None).await?;
                return Ok(OptimizationStopReason::Completed);
            }
            self.lifecycle.phase(Phase::Working).await?;
            let mut plan_trigger = self.triggers();
            plan_trigger["baseline"] = serde_json::to_value(&baseline)?;
            plan_trigger["change_request_id"] = json!(baseline.change_request_id);
            let plan: PlanOutput = serde_json::from_value(self.step("plan", plan_trigger).await?)
                .context(
                "planner must return PlanOutput {decision: propose, plan_id} or {decision: none}",
            )?;
            let plan_id = match &plan {
                PlanOutput::None => {
                    if self.is_software() && baseline.change_request_id.is_some() {
                        anyhow::bail!("planner returned no work despite the current reconciled Change Request");
                    }
                    self.lifecycle
                        .complete_cycle("no_actionable_work", None)
                        .await?;
                    return Ok(
                        if self.lifecycle.journal.software_work.blocked().is_empty() {
                            OptimizationStopReason::NoActionableWork
                        } else {
                            OptimizationStopReason::NeedsIntervention
                        },
                    );
                }
                PlanOutput::Propose { plan_id, .. } if !plan_id.trim().is_empty() => {
                    plan_id.clone()
                }
                _ => anyhow::bail!("planner proposed an empty Plan identity"),
            };
            if !plan_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                anyhow::bail!("Plan identity must contain only ASCII letters, numbers, '-' or '_'");
            }
            self.lifecycle.journal.plan_id = Some(plan_id.clone());
            let cr_id = if self.is_software() {
                let PlanOutput::Propose {
                    change_request_id, ..
                } = &plan
                else {
                    unreachable!()
                };
                if change_request_id != &baseline.change_request_id {
                    anyhow::bail!("planner Change Request does not match the current grade/reconciliation output");
                }
                let cr = change_request_id.as_deref().filter(|id| !id.trim().is_empty())
                    .context("software-improvement requires an explicit reconciled Change Request identity")?;
                self.failure_limit()?;
                self.lifecycle
                    .journal
                    .software_work
                    .prepare(self.lifecycle.store.as_ref(), cr, &plan_id)
                    .await?;
                Some(cr.to_owned())
            } else {
                None
            };
            self.lifecycle.save()?;
            // A separate, durable work claim prevents the same logical Plan
            // being executed by a later run or under a different context binding.
            // Completed work markers are intentionally retained for deduplication.
            let plan_claim = super::ownership::RunClaim::acquire(
                &self
                    .executor
                    .state_dir
                    .join("optimize/plan-claims")
                    .join(&plan_id),
                &self.binding.run_id,
            )?;
            let work_claim = if let Some(cr) = &cr_id {
                let key = newton_core::workflow::state::compute_sha256_hex(cr.as_bytes());
                Some(super::ownership::RunClaim::acquire(
                    &self
                        .executor
                        .state_dir
                        .join("optimize/work-claims")
                        .join(key),
                    &self.binding.run_id,
                )?)
            } else {
                None
            };
            if cr_id.is_some() {
                self.lifecycle
                    .store
                    .patch_plan(
                        &plan_id,
                        newton_types::PatchPlanBody {
                            status: Some("running".into()),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| anyhow!("persist running Plan: {}", e.message))?;
            }
            let mut trigger = self.triggers();
            trigger["plan"] = serde_json::to_value(&plan)?;
            trigger["plan_id"] = json!(plan_id);
            let developed =
                serde_json::from_value::<DevelopOutput>(self.step("develop", trigger).await?)
                    .context("develop must return a candidate or an explicit reconciled failure")?;
            let candidate = match (developed.candidate, developed.failure) {
                (Some(candidate), None) => candidate,
                (None, Some(failure)) => {
                    let cr = cr_id.as_deref().context(
                        "known-safe failed work requires the software-improvement strategy",
                    )?;
                    let limit = self.failure_limit()?;
                    let quarantined = self
                        .lifecycle
                        .journal
                        .software_work
                        .fail(cr, &plan_id, failure, limit)?;
                    self.lifecycle.save()?;
                    self.lifecycle
                        .journal
                        .software_work
                        .persist_failure(
                            self.lifecycle.store.as_ref(),
                            cr,
                            &plan_id,
                            self.lifecycle.journal.execution_id.clone(),
                        )
                        .await?;
                    self.lifecycle
                        .complete_cycle(
                            if quarantined {
                                "quarantined"
                            } else {
                                "retryable_failure"
                            },
                            None,
                        )
                        .await?;
                    if let Some(claim) = work_claim {
                        claim.release()?;
                    }
                    drop(plan_claim); // Retain the durable per-Plan no-replay marker.
                    if once {
                        return Ok(OptimizationStopReason::CycleComplete);
                    }
                    continue;
                }
                _ => {
                    anyhow::bail!("develop must return exactly one candidate or reconciled failure")
                }
            };
            if candidate.id != format!("{}-{}", self.binding.run_id, self.lifecycle.journal.cycle)
                || candidate.created_under_revision != self.binding.requirements.revision
            {
                anyhow::bail!("develop returned a Candidate belonging to another cycle/revision");
            }
            self.lifecycle.phase(Phase::Evaluating).await?;
            let mut trigger = self.triggers();
            trigger["stage"] = json!("candidate");
            trigger["candidate"] = serde_json::to_value(&candidate)?;
            let evaluated = self.grade(trigger).await?;
            if evaluated.candidate != candidate {
                anyhow::bail!("grade replaced the candidate identity");
            }
            let evaluation = evaluated.evaluation;
            self.lifecycle.journal.open_findings = evaluated.open_findings;
            self.validate_cycle(&evaluation)?;
            self.lifecycle.journal.candidate = Some(serde_json::to_value(&candidate)?);
            self.lifecycle.journal.evidence = Some(serde_json::to_value(&evaluation)?);
            self.lifecycle.phase(Phase::Evaluated).await?;
            self.accept_evaluated(candidate, evaluation).await?;
            self.finish_software_candidate().await?;
            if let Some(claim) = work_claim {
                claim.release()?;
            }
            if let Some(reason) = self.lifecycle.journal.threshold_history.stop {
                return Ok(reason);
            }
            let completion = self
                .outcome(OptimizationStopReason::ResourceLimit, Vec::new())?
                .completion;
            if completion.status == CheckStatus::Satisfied {
                return Ok(OptimizationStopReason::Completed);
            }
            if once {
                return Ok(OptimizationStopReason::CycleComplete);
            }
            tokio::time::sleep(std::time::Duration::from_secs(
                poll_seconds.min(self.remaining_seconds()),
            ))
            .await;
        }
    }

    async fn finish_software_candidate(&mut self) -> Result<()> {
        if !self.is_software() {
            return Ok(());
        }
        let cr = self
            .lifecycle
            .journal
            .change_request_id
            .as_ref()
            .context("selected Change Request missing")?
            .clone();
        let plan = self
            .lifecycle
            .journal
            .plan_id
            .as_ref()
            .context("selected Plan missing")?
            .clone();
        let accepted = self
            .lifecycle
            .journal
            .accepted
            .as_ref()
            .is_some_and(|result| {
                self.lifecycle
                    .journal
                    .candidate
                    .as_ref()
                    .is_some_and(|candidate| result["candidate"]["id"] == candidate["id"])
            });
        if !accepted {
            let evaluation = self
                .lifecycle
                .journal
                .evidence
                .as_ref()
                .and_then(|evidence| evidence["id"].as_str())
                .context("rejected software candidate requires evaluated evidence")?
                .to_owned();
            let limit = self.failure_limit()?;
            self.lifecycle.journal.software_work.reject_candidate(
                &cr,
                &plan,
                &evaluation,
                limit,
            )?;
            self.lifecycle.save()?;
            return self
                .lifecycle
                .journal
                .software_work
                .persist_failure(
                    self.lifecycle.store.as_ref(),
                    &cr,
                    &plan,
                    self.lifecycle.journal.execution_id.clone(),
                )
                .await;
        }
        self.lifecycle
            .store
            .patch_plan(
                &plan,
                newton_types::PatchPlanBody {
                    status: Some("complete".into()),
                    execution_id: self.lifecycle.journal.execution_id.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| anyhow!("persist completed Plan: {}", e.message))?;
        self.lifecycle
            .journal
            .software_work
            .work
            .get_mut(&cr)
            .context("selected work missing")?
            .completed = true;
        self.lifecycle.save()
    }

    fn validate_cycle(&self, evidence: &CandidateEvaluation) -> Result<()> {
        if evidence.cycle != self.lifecycle.journal.cycle {
            anyhow::bail!("evaluation belongs to another Optimize Cycle");
        }
        Ok(())
    }

    async fn grade(&mut self, triggers: Value) -> Result<GradeOutput> {
        let repeats = match self.requirements().comparison {
            ComparisonPolicy::Exact => 1,
            ComparisonPolicy::Repeated { samples, .. } => samples,
        };
        let mut combined: Option<GradeOutput> = None;
        for sample in 0..repeats {
            let mut input = triggers.clone();
            input["sample_index"] = json!(sample);
            let output: GradeOutput = serde_json::from_value(self.step("grade", input).await?)
                .context(
                    "grade must return GradeOutput {candidate, evaluation, change_request_id?}",
                )?;
            self.validate_cycle(&output.evaluation)?;
            if output.evaluation.run_id != self.binding.run_id
                || output.evaluation.requirements_revision != self.binding.requirements.revision
            {
                anyhow::bail!("grade evidence belongs to a different run or requirements revision");
            }
            if self.is_software() {
                if let ObjectiveMode::Thresholds { objectives } = &self.requirements().objective {
                    let counts = output.open_findings.as_ref().context("software-improvement threshold grading requires per-objective open_findings counts")?;
                    if counts.len() != objectives.len()
                        || objectives
                            .iter()
                            .any(|objective| !counts.contains_key(&objective.objective.id))
                    {
                        anyhow::bail!("software-improvement threshold grading requires complete per-objective open_findings counts");
                    }
                }
            }
            for measurement in output.evaluation.measurements.values() {
                match measurement {
                    ObjectiveMeasurement::Produced { samples, .. } if samples.len() == 1 => {}
                    ObjectiveMeasurement::Error { message } => anyhow::bail!("evaluator failed: {message}"),
                    _ => anyhow::bail!("each evaluator invocation must emit exactly one sample; the driver owns repeated evaluation"),
                }
            }
            if let Some(first) = combined.as_mut() {
                if first.candidate != output.candidate
                    || first.change_request_id != output.change_request_id
                    || first.open_findings != output.open_findings
                    || first.evaluation.evaluator_revisions != output.evaluation.evaluator_revisions
                    || first.evaluation.artifact_id != output.evaluation.artifact_id
                    || first.evaluation.base_artifact_id != output.evaluation.base_artifact_id
                    || first.evaluation.candidate_id != output.evaluation.candidate_id
                    || first
                        .evaluation
                        .measurements
                        .keys()
                        .ne(output.evaluation.measurements.keys())
                {
                    anyhow::bail!("repeated evaluator invocations changed candidate, objective or evaluator identity");
                }
                for (id, value) in output.evaluation.measurements {
                    match (first.evaluation.measurements.get_mut(&id), value) {
                        (
                            Some(ObjectiveMeasurement::Produced {
                                measurement,
                                samples,
                            }),
                            ObjectiveMeasurement::Produced {
                                measurement: kind,
                                samples: next,
                            },
                        ) if *measurement == kind => samples.extend(next),
                        _ => anyhow::bail!("repeated measurement changed units or kind"),
                    }
                }
                merge_checks(
                    &mut first.evaluation.constraints,
                    output.evaluation.constraints,
                )?;
                merge_checks(
                    &mut first.evaluation.completion_checks,
                    output.evaluation.completion_checks,
                )?;
            } else {
                combined = Some(output);
            }
        }
        combined.context("no evaluator invocation completed")
    }

    async fn accept_evaluated(
        &mut self,
        candidate: Candidate,
        mut evaluation: CandidateEvaluation,
    ) -> Result<()> {
        while let Some(activation) =
            super::control::activate_pending_owned(&mut self.lifecycle, &self.executor.state_dir)
                .await?
        {
            self.binding = activation.binding;
            self.regrade_incumbent(activation.incumbent).await?;
            let mut trigger = self.triggers();
            trigger["stage"] = json!("candidate");
            trigger["candidate_id"] = json!(candidate.id);
            trigger["candidate"] = serde_json::to_value(&candidate)?;
            let refreshed = self.grade(trigger).await?;
            if refreshed.candidate != candidate {
                anyhow::bail!("requirements regrade replaced the evaluated candidate identity");
            }
            evaluation = refreshed.evaluation;
            self.lifecycle.journal.open_findings = refreshed.open_findings;
            self.lifecycle.journal.evidence = Some(serde_json::to_value(&evaluation)?);
            self.lifecycle.phase(Phase::Evaluated).await?;
        }
        let incumbent: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .accepted
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        let decision = newton_core::optimization::evaluate_candidate(
            &self.binding.run_id,
            &self.binding.requirements,
            &candidate,
            &evaluation,
            incumbent.as_ref(),
        )?;
        if self
            .lifecycle
            .journal
            .threshold_history
            .observe(
                &self.binding.requirements,
                &evaluation,
                false,
                self.lifecycle.journal.open_findings.as_ref(),
            )?
            .is_some()
        {
            self.lifecycle
                .complete_cycle("threshold_stop", None)
                .await?;
            return Ok(());
        }
        if let Some(accepted) = decision.accepted_result {
            self.lifecycle.journal.accepted = Some(serde_json::to_value(accepted)?);
            self.lifecycle.complete_cycle("accepted", None).await?;
        } else {
            self.lifecycle.complete_cycle("rejected", None).await?;
        }
        Ok(())
    }

    async fn regrade_incumbent(&mut self, incumbent: Option<AcceptedResult>) -> Result<()> {
        let Some(previous) = incumbent else {
            return Ok(());
        };
        let mut trigger = self.triggers();
        trigger["stage"] = json!("candidate");
        trigger["evaluation_purpose"] = json!("incumbent_revalidation");
        trigger["candidate_id"] = json!(previous.candidate.id);
        trigger["candidate"] = serde_json::to_value(&previous.candidate)?;
        let refreshed = self.grade(trigger).await?;
        if refreshed.candidate != previous.candidate {
            anyhow::bail!("requirements regrade replaced the accepted incumbent identity");
        }
        let qualification = newton_core::optimization::evaluate_candidate(
            &self.binding.run_id,
            &self.binding.requirements,
            &previous.candidate,
            &refreshed.evaluation,
            None,
        )?;
        self.lifecycle.journal.threshold_history.observe(
            &self.binding.requirements,
            &refreshed.evaluation,
            true,
            refreshed.open_findings.as_ref(),
        )?;
        self.lifecycle.journal.accepted = qualification
            .accepted_result
            .map(serde_json::to_value)
            .transpose()?;
        self.lifecycle.save()
    }

    fn outcome(
        &self,
        stop_reason: OptimizationStopReason,
        mut diagnostics: Vec<String>,
    ) -> Result<OptimizationOutcome> {
        let accepted_result: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .accepted
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        diagnostics.extend(self.lifecycle.journal.threshold_history.diagnostics.clone());
        Ok(newton_core::optimization::build_outcome(
            &self.binding.run_id,
            &self.binding.requirements,
            accepted_result.as_ref(),
            newton_core::optimization::OutcomeContext {
                stop_reason,
                historical_result_ids: self
                    .lifecycle
                    .journal
                    .accepted_history
                    .iter()
                    .filter_map(|result| result["candidate"]["id"].as_str().map(str::to_owned))
                    .collect(),
                blocked_work: self.lifecycle.journal.software_work.blocked(),
                usage: ResourceUsage {
                    elapsed_seconds: self.elapsed(),
                    cycles: self.lifecycle.journal.cycle,
                    work: self.lifecycle.journal.work_count,
                    evaluations: self.lifecycle.journal.evaluation_count,
                },
                diagnostics,
            },
        )?)
    }
}

fn refresh_incumbent(
    journal: &mut Journal,
    active: &RequirementsRevision,
    qualified: Option<AcceptedResult>,
    baseline: &Candidate,
) -> Result<()> {
    let previous: Option<AcceptedResult> = journal
        .accepted
        .clone()
        .map(serde_json::from_value)
        .transpose()?;
    let Some(previous) = previous else {
        journal.accepted = qualified.map(serde_json::to_value).transpose()?;
        return Ok(());
    };
    let same_artifact = previous.candidate.id == baseline.id
        && previous.candidate.artifact_id == baseline.artifact_id
        && previous.candidate.base_artifact_id == baseline.base_artifact_id;
    let current = newton_core::optimization::evaluate_candidate(
        &journal.run_id,
        active,
        &previous.candidate,
        &previous.evaluation,
        None,
    )
    .is_ok_and(|decision| decision.accepted_result.is_some());
    if same_artifact || !current {
        journal
            .accepted_history
            .push(serde_json::to_value(&previous)?);
        journal.accepted = if same_artifact {
            qualified
                .map(|mut result| {
                    result.candidate = previous.candidate;
                    serde_json::to_value(result)
                })
                .transpose()?
        } else {
            // An unrelated baseline cannot silently replace a retained artifact.
            None
        };
    }
    Ok(())
}

fn merge_checks(
    first: &mut std::collections::BTreeMap<String, CheckEvidence>,
    next: std::collections::BTreeMap<String, CheckEvidence>,
) -> Result<()> {
    if first.keys().ne(next.keys()) {
        anyhow::bail!("repeated evaluation omitted or changed checks");
    }
    for (id, check) in next {
        let prior = first.get_mut(&id).context("repeated check missing")?;
        if prior.evaluator != check.evaluator || prior.judged_by != check.judged_by {
            anyhow::bail!("repeated check changed evaluator or reviewer");
        }
        prior.status = match (prior.status, check.status) {
            (CheckStatus::Violated, _) | (_, CheckStatus::Violated) => CheckStatus::Violated,
            (CheckStatus::Unknown, _) | (_, CheckStatus::Unknown) => CheckStatus::Unknown,
            _ => CheckStatus::Satisfied,
        };
        prior.evidence.extend(check.evidence);
    }
    Ok(())
}

fn resolve_reference(root: &Path, reference: &str) -> Result<PathBuf> {
    let path = root
        .join(reference)
        .canonicalize()
        .with_context(|| format!("resolve optimization workflow {reference}"))?;
    Ok(path)
}

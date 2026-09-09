//! Native, definition-bound workflow coordinator.

use super::envelopes::{DevelopOutput, GradeOutput, PlanOutput, PromoteOutput};
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
    executor: WorkflowExecutor,
    lifecycle: Lifecycle,
    started: Instant,
    previous_elapsed: u64,
}

impl NativeDriver {
    pub async fn start(
        binding: BoundOptimizationDefinition,
        definition_root: PathBuf,
        state_dir: PathBuf,
        events: broadcast::Sender<BroadcastEvent>,
    ) -> Result<Self> {
        let workspace = PathBuf::from(&binding.context.root).canonicalize()?;
        for role in ["grade", "plan", "develop"] {
            if !binding.definition.workflows.contains_key(role) {
                anyhow::bail!("native software strategy requires workflow role '{role}'");
            }
        }
        if binding.definition.strategy != "software-improvement" {
            anyhow::bail!(
                "unsupported optimization strategy {}; available: software-improvement",
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
            store,
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
        lifecycle.save()?;
        let driver = Self {
            binding,
            definition_root,
            executor: WorkflowExecutor {
                workspace,
                state_dir,
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
        let lifecycle = Lifecycle::resume(journal, store, events, &state_dir, &workspace).await?;
        Ok(Self {
            binding,
            definition_root,
            executor: WorkflowExecutor {
                workspace,
                state_dir,
            },
            lifecycle,
            started: Instant::now(),
            previous_elapsed,
        })
    }

    pub async fn run(mut self, once: bool, poll_seconds: u64) -> Result<OptimizationOutcome> {
        let result = self.run_cycles(once, poll_seconds).await;
        match result {
            Ok(reason) => {
                let outcome = self.outcome(reason, Vec::new())?;
                let status = match reason {
                    OptimizationStopReason::Completed => "converged",
                    OptimizationStopReason::NoActionableWork => "no_actionable_work",
                    OptimizationStopReason::CycleComplete => "cycle_complete",
                    _ => "max_cycles",
                };
                self.lifecycle
                    .finish(status, serde_json::to_value(&outcome)?, true)
                    .await?;
                Ok(outcome)
            }
            Err(error) => {
                let outcome = self.outcome(
                    OptimizationStopReason::OperationalFailure,
                    vec![error.to_string()],
                )?;
                // A failure during external work has an unknown side-effect
                // outcome. Persist the failure but retain ownership for review.
                self.lifecycle
                    .finish("failed", serde_json::to_value(outcome)?, false)
                    .await?;
                Err(error)
            }
        }
    }

    fn requirements(&self) -> &OptimizationRequirements {
        &self.binding.requirements.requirements
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
            "parameters": self.binding.parameters,
            "accepted_result": self.lifecycle.journal.accepted,
        })
    }

    async fn step(&mut self, role: &str, mut triggers: Value) -> Result<Value> {
        let reference = self
            .binding
            .definition
            .workflows
            .get(role)
            .with_context(|| format!("missing workflow role {role}"))?;
        let path = resolve_reference(&self.definition_root, reference)?;
        if role == "grade" {
            if self.lifecycle.journal.evaluation_count
                >= self.requirements().resource_limits.max_evaluations
            {
                anyhow::bail!("evaluation budget exhausted before complete evidence was produced");
            }
            self.lifecycle.journal.evaluation_count += 1;
        } else {
            if self.lifecycle.journal.work_count >= self.requirements().resource_limits.max_work {
                anyhow::bail!("work budget exhausted before {role}");
            }
            self.lifecycle.journal.work_count += 1;
        }
        triggers["role"] = json!(role);
        self.lifecycle.save()?;
        let summary = self
            .executor
            .execute(
                &path,
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
            self.accept_evaluated(candidate, evaluation).await?;
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
                    self.lifecycle
                        .complete_cycle("no_actionable_work", None)
                        .await?;
                    return Ok(OptimizationStopReason::NoActionableWork);
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
            self.lifecycle.save()?;
            // A separate, durable work claim prevents the same logical Plan
            // being executed by a later run or under a different context binding.
            // Completed work markers are intentionally retained for deduplication.
            let _plan_claim = super::ownership::RunClaim::acquire(
                &self
                    .executor
                    .state_dir
                    .join("optimize/plan-claims")
                    .join(&plan_id),
                &self.binding.run_id,
            )?;
            let mut trigger = self.triggers();
            trigger["plan"] = serde_json::to_value(&plan)?;
            trigger["plan_id"] = json!(plan_id);
            let candidate =
                serde_json::from_value::<DevelopOutput>(self.step("develop", trigger).await?)
                    .context("develop must return DevelopOutput {candidate}")?
                    .candidate;
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
            self.validate_cycle(&evaluation)?;
            self.lifecycle.journal.candidate = Some(serde_json::to_value(&candidate)?);
            self.lifecycle.journal.evidence = Some(serde_json::to_value(&evaluation)?);
            self.lifecycle.phase(Phase::Evaluated).await?;
            self.accept_evaluated(candidate, evaluation).await?;
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
            for measurement in output.evaluation.measurements.values() {
                match measurement {
                    ObjectiveMeasurement::Produced { samples, .. } if samples.len() == 1 => {}
                    ObjectiveMeasurement::Error { message } => anyhow::bail!("evaluator failed: {message}"),
                    _ => anyhow::bail!("each evaluator invocation must emit exactly one sample; the driver owns repeated evaluation"),
                }
            }
            if let Some(first) = combined.as_mut() {
                if first.candidate != output.candidate
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
        evaluation: CandidateEvaluation,
    ) -> Result<()> {
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
        if let Some(accepted) = decision.accepted_result {
            if self.binding.definition.workflows.contains_key("promote") {
                newton_core::optimization::authorize_action(
                    &self.binding.requirements.authority,
                    ExecutionAction::Merge,
                )?;
                self.lifecycle.phase(Phase::Promoting).await?;
                let mut trigger = self.triggers();
                trigger["accepted_result"] = serde_json::to_value(&accepted)?;
                let promoted: PromoteOutput =
                    serde_json::from_value(self.step("promote", trigger).await?)
                        .context("promote must return actual artifact_id and base_artifact_id")?;
                newton_core::optimization::validate_promotion(
                    &self.binding.run_id,
                    &self.binding.requirements,
                    &accepted,
                    &promoted.artifact_id,
                    &promoted.base_artifact_id,
                )?;
            }
            self.lifecycle.journal.accepted = Some(serde_json::to_value(accepted)?);
            self.lifecycle.complete_cycle("accepted", None).await?;
        } else {
            self.lifecycle.complete_cycle("rejected", None).await?;
        }
        Ok(())
    }

    fn outcome(
        &self,
        stop_reason: OptimizationStopReason,
        diagnostics: Vec<String>,
    ) -> Result<OptimizationOutcome> {
        let accepted_result: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .accepted
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
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
                blocked_work: Vec::new(),
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

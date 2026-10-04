//! Native, definition-bound domain-neutral optimization coordinator.

use super::{
    lifecycle::{ActiveDispatch, Journal, Lifecycle, Phase},
    workflow::WorkflowExecutor,
};
use anyhow::{Context, Result};
use newton_types::{optimization::*, BroadcastEvent};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::sync::broadcast;

struct Evaluated {
    combined: EvaluationOutput,
    invocations: Vec<EvaluationOutput>,
}

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
        if !binding.definition.workflows.contains_key("propose") {
            anyhow::bail!("generic optimization requires workflow role 'propose'");
        }
        let mut lifecycle = Lifecycle::start(
            events,
            &state_dir,
            &workspace,
            &binding.run_id,
            serde_json::to_value(&binding)?,
        )?;
        lifecycle.journal.definition_root = definition_root.clone();
        lifecycle.journal.definition_snapshot = Some(definition_snapshot);
        lifecycle.save()?;
        let projection_report =
            super::projection::prepare(&mut lifecycle, &workspace, &binding.requirements.authority);
        super::projection::report(&projection_report);
        Ok(Self {
            binding,
            definition_root,
            runtime,
            executor: WorkflowExecutor {
                workspace,
                state_dir,
            },
            lifecycle,
            started: Instant::now(),
            previous_elapsed: 0,
        })
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
        let lifecycle = Lifecycle::resume(journal, events, &state_dir, &workspace)?;
        Ok(Self {
            binding,
            definition_root,
            runtime,
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
                self.finish_and_project(serde_json::to_value(&outcome)?, true)
                    .await?;
                Ok(outcome)
            }
            Err(error) => {
                if error.is::<super::stop::Cancelled>()
                    || error.is::<super::stop::ResourceExhausted>()
                {
                    let (reason, safe) = if let Some(limit) =
                        error.downcast_ref::<super::stop::ResourceExhausted>()
                    {
                        (OptimizationStopReason::ResourceLimit, !limit.uncertain)
                    } else {
                        (
                            OptimizationStopReason::Cancelled,
                            matches!(
                                self.lifecycle.journal.phase,
                                Phase::Ready | Phase::CycleComplete
                            ),
                        )
                    };
                    let outcome = self.outcome(reason, vec![error.to_string()])?;
                    self.finish_and_project(serde_json::to_value(&outcome)?, safe)
                        .await?;
                    return Ok(outcome);
                }
                let outcome = self.outcome(
                    OptimizationStopReason::OperationalFailure,
                    vec![error.to_string()],
                )?;
                self.finish_and_project(serde_json::to_value(outcome)?, false)
                    .await?;
                Err(error)
            }
        }
    }

    async fn finish_and_project(&mut self, outcome: Value, safe_to_release: bool) -> Result<()> {
        self.lifecycle.finish(outcome, safe_to_release)?;
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
            self.executor.state_dir.clone(),
            self.lifecycle.events.clone(),
        )
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

    fn triggers(&self) -> Result<Value> {
        Ok(json!({
            "workspace": self.executor.workspace,
            "run_id": self.binding.run_id,
            "cycle": self.lifecycle.journal.cycle,
            "candidate_id": format!("{}-{}", self.binding.run_id, self.lifecycle.journal.cycle),
            "requirements_revision": self.binding.requirements.revision,
            "requirements": self.requirements(),
            "parameters": self.binding.requirements.parameters,
            "retained_result": self.lifecycle.journal.retained,
            "accepted_result": self.lifecycle.journal.accepted,
            "previous_attempts": self.previous_attempts()?,
        }))
    }

    fn previous_attempts(&self) -> Result<Vec<Value>> {
        let directory = self
            .executor
            .state_dir
            .join("optimize")
            .join(&self.binding.run_id)
            .join("cycles");
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Ok(Vec::new());
        };
        let mut paths = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .enumerate()
            .map(|(index, path)| {
                let record: OptimizationCycleRecord = serde_json::from_slice(
                    &std::fs::read(&path)
                        .with_context(|| format!("read prior Cycle {}", path.display()))?,
                )
                .with_context(|| format!("parse prior Cycle {}", path.display()))?;
                anyhow::ensure!(
                    record.run_id == self.binding.run_id
                        && record.cycle == index.saturating_add(1) as u64,
                    "prior Cycle history is non-contiguous or belongs to another run"
                );
                Ok(serde_json::to_value(record)?)
            })
            .collect()
    }

    async fn step(&mut self, role: &str, mut triggers: Value) -> Result<(Value, String)> {
        let reference = if role == "evaluate" {
            super::workflow::evaluator_reference(self.requirements())?
        } else {
            self.binding
                .definition
                .workflows
                .get(role)
                .with_context(|| format!("missing workflow role {role}"))?
        };
        let path = resolve_reference(&self.definition_root, reference)?;
        let document = self.runtime.workflow(reference)?;
        if role == "evaluate" {
            triggers["evaluator_workflow"] = json!(reference);
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
        self.lifecycle.journal.active_dispatch = Some(ActiveDispatch {
            id: dispatch,
            role: role.into(),
            started_at: chrono::Utc::now().to_rfc3339(),
        });
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
        let execution_id = summary.execution_id.to_string();
        self.lifecycle.journal.active_dispatch = None;
        self.lifecycle.save()?;
        let result = summary.result.with_context(|| {
            format!("{role} workflow must expose an explicit io.result_map; absent output is not no work")
        })?;
        Ok((result, execution_id))
    }

    async fn run_cycles(
        &mut self,
        once: bool,
        poll_seconds: u64,
    ) -> Result<OptimizationStopReason> {
        if self.lifecycle.journal.phase == Phase::Evaluated {
            self.decide_current_candidate()?;
            let reason = self.post_decision_stop(once)?;
            if let Some(reason) = reason {
                return Ok(reason);
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
            self.begin_cycle()?;
            let mut baseline_trigger = self.triggers()?;
            baseline_trigger["stage"] = json!("baseline");
            if let Some(retained) = &self.lifecycle.journal.retained {
                baseline_trigger["candidate_id"] = retained["candidate"]["id"].clone();
                baseline_trigger["candidate"] = retained["candidate"].clone();
            } else {
                baseline_trigger["candidate_id"] =
                    json!(format!("{}-baseline", self.binding.run_id));
            }
            let baseline = self.evaluate(baseline_trigger).await?;
            self.lifecycle.journal.baseline_evaluations = baseline.invocations.clone();
            self.validate_cycle(&baseline.combined.evaluation)?;
            self.qualify_baseline(&baseline.combined)?;
            self.lifecycle.journal.evidence =
                Some(serde_json::to_value(&baseline.combined.evaluation)?);
            self.lifecycle.save()?;
            if self.current_completion()?.status == CheckStatus::Satisfied {
                self.lifecycle
                    .complete_cycle(CycleStatus::Completed, Vec::new())?;
                return Ok(OptimizationStopReason::Completed);
            }

            self.lifecycle.phase(Phase::Working)?;
            let mut proposal_trigger = self.triggers()?;
            proposal_trigger["baseline"] = serde_json::to_value(&baseline.combined)?;
            let (value, _) = self.step("propose", proposal_trigger).await?;
            let proposal: ProposalOutput = serde_json::from_value(value)
                .context("propose must return a typed ProposalOutput decision")?;
            self.validate_proposal(&proposal, &baseline.invocations)?;
            self.lifecycle.journal.proposal = Some(proposal.clone());
            self.lifecycle.save()?;
            let candidate = match proposal.clone() {
                ProposalOutput::None { reason } => {
                    self.lifecycle
                        .complete_cycle(CycleStatus::NoActionableWork, vec![reason])?;
                    return Ok(OptimizationStopReason::NoActionableWork);
                }
                ProposalOutput::Failed { failure, .. } => {
                    self.record_safe_failure(&failure)?;
                    self.lifecycle
                        .complete_cycle(CycleStatus::FailedSafely, vec![failure.reason])?;
                    if let Some(reason) = self.post_decision_stop(once)? {
                        return Ok(reason);
                    }
                    continue;
                }
                ProposalOutput::Candidate { candidate, .. } => candidate,
                ProposalOutput::Execute { .. } => {
                    let mut execution_trigger = self.triggers()?;
                    execution_trigger["proposal"] = serde_json::to_value(&proposal)?;
                    let (value, execution_id) = self.step("execute", execution_trigger).await?;
                    self.lifecycle.journal.execution_id = Some(execution_id);
                    let output: ExecutionOutput = serde_json::from_value(value)
                        .context("execute must return a typed ExecutionOutput")?;
                    self.lifecycle.journal.execution_output = Some(output.clone());
                    self.lifecycle.save()?;
                    match output {
                        ExecutionOutput::Candidate { candidate } => candidate,
                        ExecutionOutput::Failed { failure } => {
                            self.record_safe_failure(&failure)?;
                            self.lifecycle
                                .complete_cycle(CycleStatus::FailedSafely, vec![failure.reason])?;
                            if let Some(reason) = self.post_decision_stop(once)? {
                                return Ok(reason);
                            }
                            continue;
                        }
                    }
                }
            };
            self.validate_candidate(&candidate)?;
            self.lifecycle.phase(Phase::Evaluating)?;
            let mut trigger = self.triggers()?;
            trigger["stage"] = json!("candidate");
            trigger["candidate"] = serde_json::to_value(&candidate)?;
            let evaluated = self.evaluate(trigger).await?;
            if evaluated.combined.candidate != candidate {
                anyhow::bail!("evaluate replaced the candidate identity");
            }
            self.validate_cycle(&evaluated.combined.evaluation)?;
            self.lifecycle.journal.candidate = Some(serde_json::to_value(&candidate)?);
            self.lifecycle.journal.candidate_evaluations = evaluated.invocations;
            self.lifecycle.journal.evidence =
                Some(serde_json::to_value(&evaluated.combined.evaluation)?);
            self.lifecycle.phase(Phase::Evaluated)?;
            self.decide_current_candidate()?;
            if let Some(reason) = self.post_decision_stop(once)? {
                return Ok(reason);
            }
            tokio::time::sleep(std::time::Duration::from_secs(
                poll_seconds.min(self.remaining_seconds()),
            ))
            .await;
        }
    }

    fn begin_cycle(&mut self) -> Result<()> {
        self.lifecycle.journal.cycle += 1;
        self.lifecycle.journal.cycle_started_at = Some(chrono::Utc::now().to_rfc3339());
        self.lifecycle.journal.active_dispatch = None;
        self.lifecycle.journal.candidate = None;
        self.lifecycle.journal.proposal = None;
        self.lifecycle.journal.execution_output = None;
        self.lifecycle.journal.baseline_evaluations.clear();
        self.lifecycle.journal.candidate_evaluations.clear();
        self.lifecycle.journal.decision = None;
        self.lifecycle.journal.execution_id = None;
        self.lifecycle.journal.evidence = None;
        self.lifecycle.phase(Phase::Evaluating)
    }

    fn qualify_baseline(&mut self, baseline: &EvaluationOutput) -> Result<()> {
        if self.lifecycle.journal.threshold_baselines.is_empty() {
            if let ObjectiveMode::Thresholds { objectives } = self.requirements().objective.clone()
            {
                for threshold in &objectives {
                    if let Some(value) =
                        measurement_value(&baseline.evaluation, &threshold.objective.id)
                    {
                        self.lifecycle
                            .journal
                            .threshold_baselines
                            .insert(threshold.objective.id.clone(), value);
                    }
                }
            }
        }
        let qualified = newton_core::optimization::evaluate_candidate(
            &self.binding.run_id,
            &self.binding.requirements,
            &baseline.candidate,
            &baseline.evaluation,
            None,
        )?
        .accepted_result;
        let retained: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .retained
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(retained) = retained {
            let same = retained.candidate.id == baseline.candidate.id
                && retained.candidate.artifact_id == baseline.candidate.artifact_id
                && retained.candidate.base_artifact_id == baseline.candidate.base_artifact_id;
            if !same {
                anyhow::bail!(
                    "baseline evaluator returned a different state than the retained result"
                );
            }
            self.lifecycle.journal.accepted = qualified
                .map(|mut result| {
                    result.candidate = retained.candidate;
                    serde_json::to_value(result)
                })
                .transpose()?;
        } else if let Some(result) = qualified {
            let value = serde_json::to_value(result)?;
            self.lifecycle.journal.accepted = Some(value.clone());
            self.lifecycle.journal.retained = Some(value);
        }
        Ok(())
    }

    fn decide_current_candidate(&mut self) -> Result<()> {
        let candidate: Candidate = serde_json::from_value(
            self.lifecycle
                .journal
                .candidate
                .clone()
                .context("evaluated phase requires candidate")?,
        )?;
        let evaluation: CandidateEvaluation = serde_json::from_value(
            self.lifecycle
                .journal
                .evidence
                .clone()
                .context("evaluated phase requires evidence")?,
        )?;
        let incumbent: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .accepted
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        let retained_exists = self.lifecycle.journal.retained.is_some();
        let decision = if retained_exists && incumbent.is_none() {
            CandidateDecision {
                candidate_id: candidate.id.clone(),
                requirements_revision: self.binding.requirements.revision,
                disposition: CandidateDisposition::Inconclusive,
                comparisons: BTreeMap::new(),
                accepted_result: None,
                reasons: vec!["retained incumbent lacks comparable current evidence; candidate cannot receive initial qualification".into()],
            }
        } else {
            newton_core::optimization::evaluate_candidate(
                &self.binding.run_id,
                &self.binding.requirements,
                &candidate,
                &evaluation,
                incumbent.as_ref(),
            )?
        };
        let improved = decision.accepted_result.is_some();
        if let Some(accepted) = &decision.accepted_result {
            let value = serde_json::to_value(accepted)?;
            if let Some(previous) = self.lifecycle.journal.retained.replace(value.clone()) {
                self.lifecycle.journal.accepted_history.push(previous);
            }
            self.lifecycle.journal.accepted = Some(value);
            self.lifecycle.journal.consecutive_no_improvement = 0;
        } else {
            self.lifecycle.journal.consecutive_no_improvement = self
                .lifecycle
                .journal
                .consecutive_no_improvement
                .checked_add(1)
                .context("stagnation counter overflow")?;
        }
        self.lifecycle.journal.decision = Some(decision.clone());
        // Acceptance is checkpointed before publishing the Cycle and evaluating stop guards.
        self.lifecycle.save()?;
        let status = if self.threshold_regressed()? {
            CycleStatus::ThresholdStop
        } else if improved {
            CycleStatus::Accepted
        } else {
            match decision.disposition {
                CandidateDisposition::Rejected => CycleStatus::Rejected,
                _ => CycleStatus::Inconclusive,
            }
        };
        self.lifecycle.complete_cycle(status, decision.reasons)
    }

    fn record_safe_failure(&mut self, failure: &AttemptFailure) -> Result<()> {
        if failure.reason.trim().is_empty()
            || failure.evidence.is_empty()
            || failure.evidence.iter().any(|item| item.trim().is_empty())
        {
            anyhow::bail!("safe attempt failure requires a reason and recovery evidence");
        }
        self.lifecycle.journal.consecutive_no_improvement = self
            .lifecycle
            .journal
            .consecutive_no_improvement
            .checked_add(1)
            .context("stagnation counter overflow")?;
        self.lifecycle.save()
    }

    fn post_decision_stop(&self, once: bool) -> Result<Option<OptimizationStopReason>> {
        if self.threshold_regressed()? {
            return Ok(Some(OptimizationStopReason::Regression));
        }
        if self.current_completion()?.status == CheckStatus::Satisfied {
            return Ok(Some(OptimizationStopReason::Completed));
        }
        let threshold_limit = match &self.requirements().objective {
            ObjectiveMode::Thresholds { objectives } => objectives
                .iter()
                .map(|objective| objective.no_progress_cycles)
                .min(),
            ObjectiveMode::Primary { .. } => None,
        };
        let stagnation_limit = threshold_limit
            .map(|limit| limit.min(self.requirements().stagnation_cycles))
            .unwrap_or(self.requirements().stagnation_cycles);
        if self.lifecycle.journal.consecutive_no_improvement >= stagnation_limit {
            return Ok(Some(OptimizationStopReason::NoProgress));
        }
        if once {
            return Ok(Some(OptimizationStopReason::CycleComplete));
        }
        Ok(None)
    }

    fn threshold_regressed(&self) -> Result<bool> {
        let ObjectiveMode::Thresholds { objectives } = &self.requirements().objective else {
            return Ok(false);
        };
        let Some(evidence) = self.lifecycle.journal.evidence.as_ref() else {
            return Ok(false);
        };
        let evaluation: CandidateEvaluation = serde_json::from_value(evidence.clone())?;
        for threshold in objectives {
            let Some(baseline) = self
                .lifecycle
                .journal
                .threshold_baselines
                .get(&threshold.objective.id)
            else {
                continue;
            };
            if let Some(value) = measurement_value(&evaluation, &threshold.objective.id) {
                if baseline - value > threshold.regression_delta {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn current_completion(&self) -> Result<CompletionAssessment> {
        let accepted: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .accepted
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        Ok(newton_core::optimization::assess_completion(
            &self.binding.run_id,
            &self.binding.requirements,
            accepted.as_ref(),
        ))
    }

    fn validate_candidate(&self, candidate: &Candidate) -> Result<()> {
        if candidate.id.trim().is_empty()
            || candidate.artifact_id.trim().is_empty()
            || candidate.base_artifact_id.trim().is_empty()
            || candidate.created_under_revision != self.binding.requirements.revision
        {
            anyhow::bail!("proposal returned an invalid Candidate identity or revision");
        }
        Ok(())
    }

    fn validate_proposal(
        &self,
        proposal: &ProposalOutput,
        baseline: &[EvaluationOutput],
    ) -> Result<()> {
        let selected = match proposal {
            ProposalOutput::Candidate {
                proposal_id,
                rationale,
                selected_observations,
                ..
            }
            | ProposalOutput::Execute {
                proposal_id,
                rationale,
                selected_observations,
                ..
            } => {
                anyhow::ensure!(
                    !proposal_id.trim().is_empty() && !rationale.trim().is_empty(),
                    "candidate/execute proposal requires identity and rationale"
                );
                selected_observations
            }
            ProposalOutput::None { reason } => {
                anyhow::ensure!(
                    !reason.trim().is_empty(),
                    "no-action proposal requires a reason"
                );
                return Ok(());
            }
            ProposalOutput::Failed {
                proposal_id,
                failure,
            } => {
                anyhow::ensure!(
                    !proposal_id.trim().is_empty()
                        && !failure.reason.trim().is_empty()
                        && !failure.evidence.is_empty(),
                    "failed proposal requires identity, reason and evidence"
                );
                return Ok(());
            }
        };
        match self.binding.definition.strategy {
            OptimizationStrategy::MeasurementDriven => {
                anyhow::ensure!(
                    selected.is_empty(),
                    "measurement-driven proposals cannot select observations"
                );
            }
            OptimizationStrategy::ObservationDriven { max_suggestions } => {
                anyhow::ensure!(
                    selected.len() <= max_suggestions as usize,
                    "proposal selected {} observations; maximum is {max_suggestions}",
                    selected.len()
                );
                let available = baseline
                    .iter()
                    .filter_map(|output| output.assessment.as_ref())
                    .flat_map(|assessment| {
                        assessment
                            .observations
                            .iter()
                            .map(move |observation| (assessment.id.clone(), observation.id.clone()))
                    })
                    .collect::<BTreeSet<_>>();
                let mut unique = BTreeSet::new();
                for reference in selected {
                    anyhow::ensure!(
                        !reference.rationale.trim().is_empty(),
                        "selected observation requires rationale"
                    );
                    let identity = (
                        reference.assessment_id.clone(),
                        reference.observation_id.clone(),
                    );
                    anyhow::ensure!(
                        available.contains(&identity),
                        "proposal selected an observation outside the current assessment"
                    );
                    anyhow::ensure!(
                        unique.insert(identity),
                        "proposal selected an observation twice"
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_cycle(&self, evidence: &CandidateEvaluation) -> Result<()> {
        if evidence.cycle != self.lifecycle.journal.cycle {
            anyhow::bail!("evaluation belongs to another Optimize Cycle");
        }
        Ok(())
    }

    async fn evaluate(&mut self, triggers: Value) -> Result<Evaluated> {
        let repeats = match self.requirements().comparison {
            ComparisonPolicy::Exact => 1,
            ComparisonPolicy::Repeated { samples, .. } => samples,
        };
        let mut invocations = Vec::new();
        let mut combined: Option<EvaluationOutput> = None;
        for sample in 0..repeats {
            let mut input = triggers.clone();
            input["sample_index"] = json!(sample);
            let (value, _) = self.step("evaluate", input).await?;
            let output: EvaluationOutput = serde_json::from_value(value).context(
                "evaluate must return EvaluationOutput {candidate, evaluation, assessment?}",
            )?;
            self.validate_cycle(&output.evaluation)?;
            if output.evaluation.run_id != self.binding.run_id
                || output.evaluation.requirements_revision != self.binding.requirements.revision
            {
                anyhow::bail!(
                    "evaluation evidence belongs to a different run or requirements revision"
                );
            }
            for measurement in output.evaluation.measurements.values() {
                match measurement {
                    ObjectiveMeasurement::Produced { samples, .. } if samples.len() == 1 => {}
                    ObjectiveMeasurement::Error { .. } => {}
                    _ => anyhow::bail!("each evaluator invocation must emit exactly one sample; the driver owns repeated evaluation"),
                }
            }
            validate_assessment(output.assessment.as_ref())?;
            invocations.push(output.clone());
            if let Some(first) = combined.as_mut() {
                merge_evaluation(first, output)?;
            } else {
                combined = Some(output);
            }
        }
        Ok(Evaluated {
            combined: combined.context("no evaluator invocation completed")?,
            invocations,
        })
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
        let retained_result: Option<AcceptedResult> = self
            .lifecycle
            .journal
            .retained
            .clone()
            .map(serde_json::from_value)
            .transpose()?;
        let mut outcome = newton_core::optimization::build_outcome(
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
        )?;
        outcome.retained_result = retained_result;
        Ok(outcome)
    }
}

fn validate_assessment(assessment: Option<&AssessmentDetails>) -> Result<()> {
    let Some(assessment) = assessment else {
        return Ok(());
    };
    anyhow::ensure!(
        !assessment.id.trim().is_empty(),
        "assessment identity must not be empty"
    );
    let mut ids = BTreeSet::new();
    for observation in &assessment.observations {
        anyhow::ensure!(
            !observation.id.trim().is_empty()
                && !observation.title.trim().is_empty()
                && !observation.rationale.trim().is_empty()
                && !observation.suggested_action.trim().is_empty(),
            "assessment observations require identity, title, rationale and suggested action"
        );
        anyhow::ensure!(
            ids.insert(&observation.id),
            "duplicate observation identity"
        );
        if let Some(resolution) = &observation.resolution {
            anyhow::ensure!(
                resolution.status != ObservationResolutionStatus::Resolved
                    || !resolution.evidence.is_empty(),
                "resolved observations require evaluator evidence"
            );
        }
    }
    if let Some(coverage) = &assessment.coverage {
        anyhow::ensure!(
            !coverage.scope.trim().is_empty(),
            "coverage scope must not be empty"
        );
        anyhow::ensure!(
            !coverage.complete || !coverage.evidence.is_empty(),
            "complete coverage requires evidence"
        );
    }
    Ok(())
}

fn merge_evaluation(first: &mut EvaluationOutput, next: EvaluationOutput) -> Result<()> {
    if first.candidate != next.candidate
        || first.evaluation.evaluator_revisions != next.evaluation.evaluator_revisions
        || first.evaluation.artifact_id != next.evaluation.artifact_id
        || first.evaluation.base_artifact_id != next.evaluation.base_artifact_id
        || first.evaluation.candidate_id != next.evaluation.candidate_id
        || first
            .evaluation
            .measurements
            .keys()
            .ne(next.evaluation.measurements.keys())
    {
        anyhow::bail!(
            "repeated evaluator invocations changed candidate, objective or evaluator identity"
        );
    }
    for (id, value) in next.evaluation.measurements {
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
            (
                Some(ObjectiveMeasurement::Error { message }),
                ObjectiveMeasurement::Error { message: next },
            ) if *message == next => {}
            _ => anyhow::bail!("repeated measurement changed units, kind or failure"),
        }
    }
    merge_checks(
        &mut first.evaluation.constraints,
        next.evaluation.constraints,
    )?;
    merge_checks(
        &mut first.evaluation.completion_checks,
        next.evaluation.completion_checks,
    )?;
    Ok(())
}

fn merge_checks(
    first: &mut BTreeMap<String, CheckEvidence>,
    next: BTreeMap<String, CheckEvidence>,
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
    root.join(reference)
        .canonicalize()
        .with_context(|| format!("resolve optimization workflow {reference}"))
}

fn measurement_value(evaluation: &CandidateEvaluation, objective: &str) -> Option<f64> {
    let ObjectiveMeasurement::Produced { samples, .. } = evaluation.measurements.get(objective)?
    else {
        return None;
    };
    (!samples.is_empty()).then(|| samples.iter().sum::<f64>() / samples.len() as f64)
}

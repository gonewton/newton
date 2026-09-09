use super::{
    decision::{acceptance_status, check_status, combine_statuses, direction, measurement_samples},
    validate_evaluation,
    validation::objective_specs,
    OptimizationError,
};
use newton_types::optimization::*;
use std::collections::BTreeMap;

/// Assess the destination independently of the strategy's stop/convergence signal.
/// Repeated measurements must all meet the target; missing evidence is unknown.
pub fn assess_completion(
    run_id: &str,
    active: &RequirementsRevision,
    accepted: Option<&AcceptedResult>,
) -> CompletionAssessment {
    let mut result = CompletionAssessment {
        status: CheckStatus::Unknown,
        criteria: BTreeMap::new(),
        reasons: Vec::new(),
    };
    let current = accepted.filter(|accepted| {
        match validate_evaluation(run_id, active, &accepted.candidate, &accepted.evaluation) {
            Ok(()) => {
                acceptance_status(
                    &active.requirements,
                    &accepted.evaluation,
                    &mut result.reasons,
                ) == CheckStatus::Satisfied
            }
            Err(error) => {
                result.reasons.push(error.to_string());
                false
            }
        }
    });
    if current.is_none() {
        result
            .reasons
            .push("no accepted result with qualifying current evidence".into());
    }
    for criterion in &active.requirements.completion {
        let (key, status) = match criterion {
            CompletionCriterion::ObjectiveTarget { objective, target } => {
                let status = current
                    .and_then(|accepted| {
                        objective_specs(&active.requirements)
                            .into_iter()
                            .find(|spec| &spec.id == objective)
                            .and_then(|spec| {
                                measurement_samples(
                                    &accepted.evaluation,
                                    spec,
                                    &active.requirements.comparison,
                                )
                                .ok()
                                .map(|samples| (spec, samples))
                            })
                    })
                    .map_or(CheckStatus::Unknown, |(spec, samples)| {
                        let passes =
                            samples
                                .iter()
                                .all(|value| match direction(&spec.measurement) {
                                    Direction::Minimize => value <= target,
                                    Direction::Maximize => value >= target,
                                });
                        if passes {
                            CheckStatus::Satisfied
                        } else {
                            CheckStatus::Violated
                        }
                    });
                (format!("objective:{objective}"), status)
            }
            CompletionCriterion::Check { constraint } => {
                let status = current.map_or(CheckStatus::Unknown, |accepted| {
                    check_status(
                        constraint,
                        accepted.evaluation.completion_checks.get(&constraint.id),
                        &mut result.reasons,
                    )
                });
                (format!("check:{}", constraint.id), status)
            }
        };
        result.criteria.insert(key, status);
    }
    // Legacy threshold mode always retains conjunction over all per-grader targets.
    if let ObjectiveMode::Thresholds { objectives } = &active.requirements.objective {
        for threshold in objectives {
            let status = current
                .and_then(|accepted| {
                    measurement_samples(
                        &accepted.evaluation,
                        &threshold.objective,
                        &active.requirements.comparison,
                    )
                    .ok()
                })
                .map_or(CheckStatus::Unknown, |samples| {
                    if samples.iter().all(|value| *value >= threshold.target) {
                        CheckStatus::Satisfied
                    } else {
                        CheckStatus::Violated
                    }
                });
            result
                .criteria
                .insert(format!("threshold:{}", threshold.objective.id), status);
        }
    }
    if !result.criteria.is_empty() {
        result.status = combine_statuses(result.criteria.values().copied());
    }
    result
}

/// Inputs from actual runtime accounting and retained history for a final report.
#[derive(Debug, Clone)]
pub struct OutcomeContext {
    /// Why execution actually stopped.
    pub stop_reason: OptimizationStopReason,
    /// Known counters; every dispatched retry/repeat must already be included.
    pub usage: ResourceUsage,
    /// Unresolved/quarantined logical work identifiers.
    pub blocked_work: Vec<String>,
    /// Historical accepted states retained without implied current validity.
    pub historical_result_ids: Vec<String>,
    /// Operational errors, recoverability references, and other diagnostics.
    pub diagnostics: Vec<String>,
}

/// Build an evidence-backed outcome. Stale accepted results remain history, not
/// current results; failure/cancellation retain their actual stopping reason.
pub fn build_outcome(
    run_id: &str,
    active: &RequirementsRevision,
    accepted: Option<&AcceptedResult>,
    context: OutcomeContext,
) -> Result<OptimizationOutcome, OptimizationError> {
    super::validate_requirements(&active.requirements)?;
    if run_id.trim().is_empty() || active.status != RequirementsRevisionStatus::Active {
        return Err(OptimizationError::InvalidOutcome(
            "an outcome requires a run identity and active requirements".into(),
        ));
    }
    let completion = assess_completion(run_id, active, accepted);
    if context.stop_reason == OptimizationStopReason::Completed
        && completion.status != CheckStatus::Satisfied
    {
        return Err(OptimizationError::InvalidOutcome(
            "completion cannot be claimed without satisfying current completion criteria".into(),
        ));
    }
    let mut diagnostics = context.diagnostics;
    let accepted_result = accepted
        .filter(|accepted| {
            match validate_evaluation(run_id, active, &accepted.candidate, &accepted.evaluation) {
                Ok(()) => {
                    acceptance_status(&active.requirements, &accepted.evaluation, &mut diagnostics)
                        == CheckStatus::Satisfied
                }
                Err(error) => {
                    diagnostics.push(error.to_string());
                    false
                }
            }
        })
        .cloned();
    let mut historical_result_ids = context.historical_result_ids;
    if accepted_result.is_none() {
        if let Some(previous) = accepted {
            if !historical_result_ids.contains(&previous.candidate.id) {
                historical_result_ids.push(previous.candidate.id.clone());
            }
        }
        diagnostics.push("no acceptable result found; this does not establish infeasibility or authorize rollback".into());
    }
    Ok(OptimizationOutcome {
        run_id: run_id.into(),
        requirements_revision: active.revision,
        requirements: active.requirements.clone(),
        stop_reason: context.stop_reason,
        completion,
        no_acceptable_result_found: accepted_result.is_none(),
        accepted_result,
        historical_result_ids,
        blocked_work: context.blocked_work,
        usage: context.usage,
        diagnostics,
    })
}

/// Report every consumed finite resource; the caller checks before each dispatch
/// and independently uses a deadline to interrupt in-flight work/evaluation.
pub fn exhausted_resources(limits: &ResourceLimits, usage: &ResourceUsage) -> Vec<&'static str> {
    [
        (
            usage.elapsed_seconds >= limits.elapsed_seconds,
            "elapsed_seconds",
        ),
        (usage.cycles >= limits.max_cycles, "cycles"),
        (usage.work >= limits.max_work, "work"),
        (usage.evaluations >= limits.max_evaluations, "evaluations"),
    ]
    .into_iter()
    .filter_map(|(exhausted, name)| exhausted.then_some(name))
    .collect()
}

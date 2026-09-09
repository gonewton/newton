use super::{validate_requirements, validation::objective_specs, OptimizationError};
use newton_types::optimization::*;
use std::collections::BTreeMap;

/// Evaluate a candidate before promotion. Initial qualification does not claim
/// improvement; ties/inconclusive evidence never replace the accepted result.
/// The caller must persist the decision and separately authorize any promotion.
pub fn evaluate_candidate(
    run_id: &str,
    active: &RequirementsRevision,
    candidate: &Candidate,
    evaluation: &CandidateEvaluation,
    accepted: Option<&AcceptedResult>,
) -> Result<CandidateDecision, OptimizationError> {
    validate_evaluation(run_id, active, candidate, evaluation)?;
    let mut decision = CandidateDecision {
        candidate_id: candidate.id.clone(),
        requirements_revision: active.revision,
        disposition: CandidateDisposition::Inconclusive,
        comparisons: BTreeMap::new(),
        accepted_result: None,
        reasons: Vec::new(),
    };
    let checks = acceptance_status(&active.requirements, evaluation, &mut decision.reasons);
    if checks != CheckStatus::Satisfied {
        if checks == CheckStatus::Violated {
            decision.disposition = CandidateDisposition::Rejected;
        }
        return Ok(decision);
    }
    if let Some(baseline) = accepted {
        if let Err(error) =
            validate_evaluation(run_id, active, &baseline.candidate, &baseline.evaluation)
        {
            decision
                .reasons
                .push(format!("accepted baseline requires re-evaluation: {error}"));
            return Ok(decision);
        }
        if acceptance_status(
            &active.requirements,
            &baseline.evaluation,
            &mut decision.reasons,
        ) != CheckStatus::Satisfied
        {
            decision.reasons.push("accepted baseline no longer qualifies; retain history and recheck before comparison".into());
            return Ok(decision);
        }
    }
    for objective in objective_specs(&active.requirements) {
        let candidate_samples =
            measurement_samples(evaluation, objective, &active.requirements.comparison)?;
        let comparison = match accepted {
            None => ComparisonOutcome::Initial,
            Some(baseline) => {
                let baseline_samples = measurement_samples(
                    &baseline.evaluation,
                    objective,
                    &active.requirements.comparison,
                )?;
                compare_samples(
                    &objective.measurement,
                    &active.requirements.comparison,
                    candidate_samples,
                    baseline_samples,
                )
            }
        };
        decision
            .comparisons
            .insert(objective.id.clone(), comparison);
    }
    let comparisons: Vec<_> = decision.comparisons.values().copied().collect();
    decision.disposition = if accepted.is_none() {
        CandidateDisposition::InitialQualification
    } else if comparisons.contains(&ComparisonOutcome::Worse) {
        CandidateDisposition::Rejected
    } else if comparisons.contains(&ComparisonOutcome::Inconclusive) {
        CandidateDisposition::Inconclusive
    } else if comparisons.contains(&ComparisonOutcome::Better) {
        CandidateDisposition::Improvement
    } else {
        CandidateDisposition::Unchanged
    };
    if matches!(
        decision.disposition,
        CandidateDisposition::InitialQualification | CandidateDisposition::Improvement
    ) {
        decision.accepted_result = Some(AcceptedResult {
            candidate: candidate.clone(),
            evaluation: evaluation.clone(),
        });
    }
    Ok(decision)
}

/// Validate all measurement identities and shapes against the active revision.
/// An operational evaluator failure is returned as an error, never convergence.
pub fn validate_evaluation(
    run_id: &str,
    active: &RequirementsRevision,
    candidate: &Candidate,
    evaluation: &CandidateEvaluation,
) -> Result<(), OptimizationError> {
    validate_requirements(&active.requirements)?;
    if active.status != RequirementsRevisionStatus::Active || active.revision == 0 {
        return evidence_error("acceptance requires an acknowledged active revision");
    }
    if evaluation.run_id != run_id
        || evaluation.requirements_revision != active.revision
        || evaluation.candidate_id != candidate.id
        || evaluation.artifact_id != candidate.artifact_id
        || evaluation.base_artifact_id != candidate.base_artifact_id
    {
        return evidence_error(
            "run, candidate artifact/base, or requirements revision does not match",
        );
    }
    if run_id.trim().is_empty()
        || evaluation.id.trim().is_empty()
        || candidate.id.trim().is_empty()
        || candidate.artifact_id.trim().is_empty()
        || candidate.base_artifact_id.trim().is_empty()
        || candidate.created_under_revision == 0
        || candidate.created_under_revision > active.revision
    {
        return evidence_error(
            "run/evidence/candidate identities must be nonempty and creation revision valid",
        );
    }
    let expected: BTreeMap<_, _> = active
        .requirements
        .evaluators
        .iter()
        .map(|(name, evaluator)| (name.clone(), evaluator.revision.clone()))
        .collect();
    if evaluation.evaluator_revisions != expected {
        return evidence_error("evaluator/rubric/input revisions differ from active requirements");
    }
    for objective in objective_specs(&active.requirements) {
        measurement_samples(evaluation, objective, &active.requirements.comparison)?;
    }
    Ok(())
}

/// Confirm that the exact evaluated state and base are the ones being promoted.
/// This does not grant permission or replace the host's atomic compare-and-swap.
pub fn validate_promotion(
    run_id: &str,
    active: &RequirementsRevision,
    accepted: &AcceptedResult,
    artifact_id: &str,
    base_artifact_id: &str,
) -> Result<(), OptimizationError> {
    validate_evaluation(run_id, active, &accepted.candidate, &accepted.evaluation)?;
    if accepted.candidate.artifact_id != artifact_id
        || accepted.candidate.base_artifact_id != base_artifact_id
    {
        return evidence_error(
            "promotion artifact or integration base changed; re-evaluation is required",
        );
    }
    let mut reasons = Vec::new();
    if acceptance_status(&active.requirements, &accepted.evaluation, &mut reasons)
        != CheckStatus::Satisfied
    {
        return evidence_error(format!(
            "promotion constraints do not qualify: {}",
            reasons.join("; ")
        ));
    }
    Ok(())
}

pub(super) fn measurement_samples<'a>(
    evaluation: &'a CandidateEvaluation,
    objective: &ObjectiveSpec,
    policy: &ComparisonPolicy,
) -> Result<&'a [f64], OptimizationError> {
    let value = evaluation.measurements.get(&objective.id).ok_or_else(|| {
        OptimizationError::InvalidEvidence(format!("missing objective {}", objective.id))
    })?;
    let (measurement, samples) = match value {
        ObjectiveMeasurement::Produced {
            measurement,
            samples,
        } => (measurement, samples),
        ObjectiveMeasurement::Error { message } => {
            return evidence_error(format!(
                "objective {} evaluation failed: {message}",
                objective.id
            ))
        }
    };
    let expected_samples = match policy {
        ComparisonPolicy::Exact => 1,
        ComparisonPolicy::Repeated { samples, .. } => *samples as usize,
    };
    if measurement != &objective.measurement || samples.len() != expected_samples {
        return evidence_error(format!(
            "objective {} has incompatible units/type or incomplete repeat samples",
            objective.id
        ));
    }
    if samples.iter().any(|value| {
        !value.is_finite()
            || (matches!(measurement, MeasurementKind::Grade { .. })
                && !(0.0..=100.0).contains(value))
    }) {
        return evidence_error(format!(
            "objective {} samples must be finite and Grades must be 0–100",
            objective.id
        ));
    }
    Ok(samples)
}

pub(super) fn acceptance_status(
    requirements: &OptimizationRequirements,
    evaluation: &CandidateEvaluation,
    reasons: &mut Vec<String>,
) -> CheckStatus {
    combine_statuses(
        requirements
            .acceptance_constraints
            .iter()
            .map(|check| check_status(check, evaluation.constraints.get(&check.id), reasons)),
    )
}

pub(super) fn check_status(
    check: &AcceptanceConstraint,
    evidence: Option<&CheckEvidence>,
    reasons: &mut Vec<String>,
) -> CheckStatus {
    let Some(evidence) = evidence else {
        reasons.push(format!("check {} has no evidence", check.id));
        return CheckStatus::Unknown;
    };
    if evidence.evaluator != check.evaluator {
        reasons.push(format!("check {} used a different evaluator", check.id));
        return CheckStatus::Unknown;
    }
    if check.human_judged
        && evidence
            .judged_by
            .as_ref()
            .is_none_or(|person| person.trim().is_empty())
    {
        reasons.push(format!(
            "check {} requires an identified human judgment",
            check.id
        ));
        return CheckStatus::Unknown;
    }
    if evidence.status != CheckStatus::Satisfied {
        reasons.push(format!("check {} is {:?}", check.id, evidence.status));
    }
    evidence.status
}

pub(super) fn combine_statuses(statuses: impl Iterator<Item = CheckStatus>) -> CheckStatus {
    statuses.fold(CheckStatus::Satisfied, |overall, item| {
        match (overall, item) {
            (CheckStatus::Violated, _) | (_, CheckStatus::Violated) => CheckStatus::Violated,
            (CheckStatus::Unknown, _) | (_, CheckStatus::Unknown) => CheckStatus::Unknown,
            _ => CheckStatus::Satisfied,
        }
    })
}

pub(super) fn direction(measurement: &MeasurementKind) -> Direction {
    match measurement {
        MeasurementKind::Numeric { direction, .. } => *direction,
        MeasurementKind::Grade { .. } => Direction::Maximize,
    }
}

fn compare_samples(
    measurement: &MeasurementKind,
    policy: &ComparisonPolicy,
    candidate: &[f64],
    baseline: &[f64],
) -> ComparisonOutcome {
    let sign = if direction(measurement) == Direction::Maximize {
        1.0
    } else {
        -1.0
    };
    match policy {
        ComparisonPolicy::Exact => {
            let candidate = candidate[0] * sign;
            let baseline = baseline[0] * sign;
            if candidate > baseline {
                ComparisonOutcome::Better
            } else if candidate < baseline {
                ComparisonOutcome::Worse
            } else {
                ComparisonOutcome::Tie
            }
        }
        ComparisonPolicy::Repeated {
            min_improvement, ..
        } => {
            let range = |samples: &[f64]| {
                samples
                    .iter()
                    .map(|value| value * sign)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| {
                        (min.min(value), max.max(value))
                    })
            };
            let (candidate_min, candidate_max) = range(candidate);
            let (baseline_min, baseline_max) = range(baseline);
            if candidate_min - baseline_max >= *min_improvement {
                ComparisonOutcome::Better
            } else if baseline_min - candidate_max >= *min_improvement {
                ComparisonOutcome::Worse
            } else {
                ComparisonOutcome::Inconclusive
            }
        }
    }
}

fn evidence_error<T>(message: impl Into<String>) -> Result<T, OptimizationError> {
    Err(OptimizationError::InvalidEvidence(message.into()))
}

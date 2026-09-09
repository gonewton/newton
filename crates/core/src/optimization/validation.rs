use super::OptimizationError;
use newton_types::optimization::*;
use std::collections::BTreeSet;

/// Parse declarative YAML (including JSON) and reject unknown/invalid fields.
/// No environment interpolation, shell sourcing, or executable callbacks occur.
pub fn parse_definition(source: &str) -> Result<OptimizationDefinition, OptimizationError> {
    let definition = serde_yaml::from_str(source)
        .map_err(|e| OptimizationError::InvalidDefinition(e.to_string()))?;
    validate_definition(&definition)?;
    Ok(definition)
}

/// Check the versioned source contract without granting execution permissions.
pub fn validate_definition(definition: &OptimizationDefinition) -> Result<(), OptimizationError> {
    if definition.schema_version != OPTIMIZATION_DEFINITION_VERSION {
        return invalid("schema_version must be 1");
    }
    for (label, value) in [
        ("id", &definition.id),
        ("revision", &definition.revision),
        ("strategy", &definition.strategy),
    ] {
        nonempty(value, label)?;
    }
    if definition.workflows.is_empty() {
        return invalid("at least one strategy workflow is required");
    }
    for (role, path) in &definition.workflows {
        nonempty(role, "workflow role")?;
        nonempty(path, "workflow reference")?;
    }
    validate_parameters(&definition.defaults)?;
    validate_requirements(&definition.requirements)
}

/// Validate standalone requirements before accepting a local revision request.
pub fn validate_requirements(
    requirements: &OptimizationRequirements,
) -> Result<(), OptimizationError> {
    let limits = &requirements.resource_limits;
    if limits.elapsed_seconds == 0
        || limits.max_cycles == 0
        || limits.max_work == 0
        || limits.max_evaluations == 0
    {
        return invalid(
            "elapsed_seconds, max_cycles, max_work, and max_evaluations must be positive",
        );
    }
    let samples = match requirements.comparison {
        ComparisonPolicy::Exact => 1,
        ComparisonPolicy::Repeated {
            samples,
            min_improvement,
        } => {
            if samples < 2 || !min_improvement.is_finite() || min_improvement <= 0.0 {
                return invalid(
                    "repeated comparison requires samples >= 2 and finite min_improvement > 0",
                );
            }
            samples
        }
    };
    for (key, evaluator) in &requirements.evaluators {
        nonempty(key, "evaluator key")?;
        nonempty(&evaluator.workflow, "evaluator workflow")?;
        nonempty(&evaluator.revision, "evaluator revision")?;
    }
    let objectives = objective_specs(requirements);
    if objectives.is_empty() {
        return invalid("at least one objective is required");
    }
    let mut ids = BTreeSet::new();
    for objective in &objectives {
        nonempty(&objective.id, "objective id")?;
        if !ids.insert(objective.id.as_str()) {
            return invalid(format!("duplicate objective {}", objective.id));
        }
        evaluator_exists(requirements, &objective.evaluator)?;
        match &objective.measurement {
            MeasurementKind::Numeric { unit, .. } => nonempty(unit, "numeric unit")?,
            MeasurementKind::Grade { dimension } => nonempty(dimension, "Grade dimension")?,
        }
    }
    // One evaluator invocation may emit several objectives. Count distinct
    // evaluators, not dimensions, when validating the minimum repeat budget.
    let evaluators: BTreeSet<_> = objectives.iter().map(|o| &o.evaluator).collect();
    if u64::from(samples).saturating_mul(evaluators.len() as u64) > limits.max_evaluations {
        return invalid("evaluation budget cannot cover one complete objective evaluation");
    }
    if let ObjectiveMode::Thresholds { objectives } = &requirements.objective {
        if !matches!(requirements.comparison, ComparisonPolicy::Exact) {
            return invalid("threshold mode uses exact per-grader comparisons");
        }
        for threshold in objectives {
            if !matches!(
                threshold.objective.measurement,
                MeasurementKind::Grade { .. }
            ) {
                return invalid("threshold objectives must use Grades");
            }
            if !threshold.target.is_finite()
                || !(0.0..=100.0).contains(&threshold.target)
                || !threshold.regression_delta.is_finite()
                || threshold.regression_delta < 0.0
                || threshold.no_progress_cycles == 0
            {
                return invalid("threshold target must be 0–100, regression_delta >= 0, and no_progress_cycles > 0");
            }
        }
    }
    let mut constraints = BTreeSet::new();
    for constraint in &requirements.acceptance_constraints {
        validate_constraint(requirements, constraint)?;
        if !constraints.insert(&constraint.id) {
            return invalid(format!("duplicate acceptance constraint {}", constraint.id));
        }
    }
    if requirements.completion.is_empty() {
        return invalid("at least one explicit completion criterion is required");
    }
    let mut completion_ids = BTreeSet::new();
    for criterion in &requirements.completion {
        let key = match criterion {
            CompletionCriterion::ObjectiveTarget { objective, target } => {
                let spec = objectives
                    .iter()
                    .find(|spec| &spec.id == objective)
                    .ok_or_else(|| {
                        OptimizationError::InvalidDefinition(format!(
                            "unknown completion objective {objective}"
                        ))
                    })?;
                if !target.is_finite()
                    || (matches!(spec.measurement, MeasurementKind::Grade { .. })
                        && !(0.0..=100.0).contains(target))
                {
                    return invalid(
                        "completion target must be finite and within Grade bounds when applicable",
                    );
                }
                format!("objective:{objective}")
            }
            CompletionCriterion::Check { constraint } => {
                validate_constraint(requirements, constraint)?;
                format!("check:{}", constraint.id)
            }
        };
        if !completion_ids.insert(key) {
            return invalid("duplicate completion criterion");
        }
    }
    for path in &requirements.execution_restrictions.protected_paths {
        nonempty(path, "protected path")?;
    }
    Ok(())
}

fn validate_constraint(
    requirements: &OptimizationRequirements,
    check: &AcceptanceConstraint,
) -> Result<(), OptimizationError> {
    nonempty(&check.id, "check id")?;
    evaluator_exists(requirements, &check.evaluator)
}

fn evaluator_exists(
    requirements: &OptimizationRequirements,
    evaluator: &str,
) -> Result<(), OptimizationError> {
    if !requirements.evaluators.contains_key(evaluator) {
        return invalid(format!("unknown evaluator {evaluator}"));
    }
    Ok(())
}

pub(super) fn objective_specs(requirements: &OptimizationRequirements) -> Vec<&ObjectiveSpec> {
    match &requirements.objective {
        ObjectiveMode::Primary { objective } => vec![objective],
        ObjectiveMode::Thresholds { objectives } => {
            objectives.iter().map(|o| &o.objective).collect()
        }
    }
}

pub(super) fn validate_parameters(
    parameters: &std::collections::BTreeMap<String, ParameterValue>,
) -> Result<(), OptimizationError> {
    for (key, value) in parameters {
        nonempty(key, "parameter name")?;
        if let ParameterValue::SecretReference { reference } = value {
            nonempty(reference, "secret reference")?;
        }
    }
    Ok(())
}

pub(super) fn nonempty(value: &str, label: &str) -> Result<(), OptimizationError> {
    if value.trim().is_empty() {
        return invalid(format!("{label} must not be empty"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, OptimizationError> {
    Err(OptimizationError::InvalidDefinition(message.into()))
}

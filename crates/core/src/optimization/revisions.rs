use super::{
    binding::{ensure_enforceable, restrict_authority},
    validate_requirements,
    validation::validate_parameters,
    EnforcementCapabilities, OptimizationError,
};
use newton_types::optimization::*;

/// Authorization supplied by the trusted local control boundary, never by the
/// candidate agent or the contents of a declarative update file.
#[derive(Debug, Clone)]
pub struct RequirementsUpdateAuthority {
    /// Authenticated local caller identity retained in revision history.
    pub actor: String,
    /// Permission to revise ordinary requirements and settings.
    pub may_update_requirements: bool,
    /// Additional permission to replace evaluator, objective, or comparison policy.
    pub may_update_evaluators: bool,
    /// Additional permission to revise action restrictions/protected paths.
    pub may_update_execution_restrictions: bool,
}

/// Boundary established by the native runtime, not a claim from an update file.
#[derive(Debug, Clone, Default)]
pub struct RevisionBoundary {
    /// True only after all affected agents, commands, and child workflows are paused.
    pub affected_work_paused: bool,
    /// Relevant completed/in-flight actions disclosed to the user.
    pub prior_actions: Vec<String>,
}

/// Both sides of a successful activation, to persist together before acknowledging.
#[derive(Debug, Clone)]
pub struct RevisionActivation {
    /// Newly authoritative requirements for all subsequent work/acceptance.
    pub active: RequirementsRevision,
    /// Previous policy retained as history without rolling back its accepted work.
    pub superseded: RequirementsRevision,
}

/// Validate a local update and produce pending state without acknowledging it.
/// A stale request cannot silently overwrite a newer acknowledged revision.
pub fn propose_revision(
    active: &RequirementsRevision,
    ceiling: &ExecutionAuthority,
    update: RequirementsUpdate,
    authority: &RequirementsUpdateAuthority,
    enforcement: &EnforcementCapabilities,
) -> Result<RequirementsRevision, OptimizationError> {
    check_active(active, update.base_revision)?;
    if !authority.may_update_requirements || authority.actor.trim().is_empty() {
        return Err(OptimizationError::Unauthorized(
            "requirements update requires an identified authorized caller".into(),
        ));
    }
    let evaluator_changed = active.requirements.evaluators != update.requirements.evaluators
        || active.requirements.objective != update.requirements.objective
        || active.requirements.comparison != update.requirements.comparison;
    if evaluator_changed && !authority.may_update_evaluators {
        return Err(OptimizationError::Unauthorized(
            "evaluator/objective/comparison changes require evaluator authority".into(),
        ));
    }
    if active.requirements.execution_restrictions != update.requirements.execution_restrictions
        && !authority.may_update_execution_restrictions
    {
        return Err(OptimizationError::Unauthorized(
            "execution restriction changes require restriction authority".into(),
        ));
    }
    validate_requirements(&update.requirements)?;
    ensure_enforceable(&update.requirements.execution_restrictions, enforcement)?;
    let mut parameters = active.parameters.clone();
    parameters.extend(update.parameter_overrides);
    validate_parameters(&parameters)?;
    let revision = active.revision.checked_add(1).ok_or_else(|| {
        OptimizationError::InvalidBinding("requirements revision counter exhausted".into())
    })?;
    Ok(RequirementsRevision {
        revision,
        base_revision: Some(active.revision),
        authority: restrict_authority(ceiling, &update.requirements.execution_restrictions),
        requirements: update.requirements,
        parameters,
        status: RequirementsRevisionStatus::Pending,
        requested_by: authority.actor.clone(),
        prior_actions: Vec::new(),
        reason: Some("awaiting runtime acknowledgment".into()),
    })
}

/// Activate an already authorized pending update only at the required boundary.
/// Persist both returned revisions before reporting activation to the user.
/// A restriction update stays pending if the host has not paused affected work.
pub fn acknowledge_revision(
    active: &RequirementsRevision,
    pending: &RequirementsRevision,
    boundary: RevisionBoundary,
    enforcement: &EnforcementCapabilities,
) -> Result<RevisionActivation, OptimizationError> {
    let base = pending.base_revision.ok_or_else(|| {
        OptimizationError::InvalidBinding("pending revision has no base revision".into())
    })?;
    check_active(active, base)?;
    if pending.status != RequirementsRevisionStatus::Pending
        || active.revision.checked_add(1) != Some(pending.revision)
    {
        return Err(OptimizationError::InvalidBinding(
            "only the next pending revision can become active".into(),
        ));
    }
    validate_requirements(&pending.requirements)?;
    ensure_enforceable(&pending.requirements.execution_restrictions, enforcement)?;
    let restrictions_changed =
        active.requirements.execution_restrictions != pending.requirements.execution_restrictions;
    if restrictions_changed && !boundary.affected_work_paused {
        return Err(OptimizationError::UnsupportedRestriction(
            "affected work has not been paused; update remains pending and is not active".into(),
        ));
    }
    let mut next = pending.clone();
    next.status = RequirementsRevisionStatus::Active;
    next.prior_actions = boundary.prior_actions;
    next.reason = None;
    let mut superseded = active.clone();
    superseded.status = RequirementsRevisionStatus::Superseded;
    Ok(RevisionActivation {
        active: next,
        superseded,
    })
}

/// Retain a pending update as explicitly rejected rather than silently dropping it.
pub fn reject_revision(
    pending: &RequirementsRevision,
    reason: impl Into<String>,
) -> Result<RequirementsRevision, OptimizationError> {
    if pending.status != RequirementsRevisionStatus::Pending {
        return Err(OptimizationError::InvalidBinding(
            "only a pending revision can be rejected".into(),
        ));
    }
    let reason = reason.into();
    if reason.trim().is_empty() {
        return Err(OptimizationError::InvalidBinding(
            "revision rejection must explain why".into(),
        ));
    }
    let mut rejected = pending.clone();
    rejected.status = RequirementsRevisionStatus::Rejected;
    rejected.reason = Some(reason);
    Ok(rejected)
}

fn check_active(active: &RequirementsRevision, base: u64) -> Result<(), OptimizationError> {
    if active.status != RequirementsRevisionStatus::Active || active.revision == 0 {
        return Err(OptimizationError::InvalidBinding(
            "requirements must have an active revision".into(),
        ));
    }
    if base != active.revision {
        return Err(OptimizationError::StaleRevision {
            expected: active.revision,
            actual: base,
        });
    }
    Ok(())
}

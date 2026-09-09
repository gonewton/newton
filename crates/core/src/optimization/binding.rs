use super::{validate_definition, validation::validate_parameters, OptimizationError};
use newton_types::optimization::*;
use std::collections::{BTreeMap, BTreeSet};

/// Enforcement the host actually supplies, not declarations taken from a file.
/// A host must not claim support based only on prompts or post-hoc grading.
#[derive(Debug, Clone, Default)]
pub struct EnforcementCapabilities {
    /// Action prohibitions enforced across all agents, commands, and nested workflows.
    pub denied_actions: BTreeSet<ExecutionAction>,
    /// Whether authoritative evaluator inputs are inaccessible to candidate writes.
    pub protected_paths: bool,
}

/// Host-supplied binding inputs; ordinary values and authority are separate.
#[derive(Debug, Clone)]
pub struct DefinitionBinding {
    /// Optimize Run identity.
    pub run_id: String,
    /// Context requiring no portfolio setup.
    pub context: OptimizationContext,
    /// Project-level ordinary values.
    pub project_parameters: BTreeMap<String, ParameterValue>,
    /// Explicit run values with highest ordinary precedence.
    pub run_overrides: BTreeMap<String, ParameterValue>,
    /// Project owner's maximum allowed actions.
    pub project_authority: ExecutionAuthority,
    /// Execution environment's maximum allowed actions.
    pub environment_authority: ExecutionAuthority,
    /// Enforceable restrictions established by the host.
    pub enforcement: EnforcementCapabilities,
}

/// Resolve a reusable definition into an immutable run snapshot without executing
/// anything or altering workspace configuration. Persist the result before use.
pub fn bind_definition(
    definition: &OptimizationDefinition,
    binding: DefinitionBinding,
) -> Result<BoundOptimizationDefinition, OptimizationError> {
    validate_definition(definition)?;
    if binding.run_id.trim().is_empty()
        || binding.context.id.trim().is_empty()
        || binding.context.root.trim().is_empty()
    {
        return Err(OptimizationError::InvalidBinding(
            "run id, context id, and root must not be empty".into(),
        ));
    }
    ensure_enforceable(
        &definition.requirements.execution_restrictions,
        &binding.enforcement,
    )?;
    let mut parameters = definition.defaults.clone();
    parameters.extend(binding.project_parameters);
    parameters.extend(binding.run_overrides);
    validate_parameters(&parameters)?;
    let authority_ceiling = ExecutionAuthority {
        allowed_actions: binding
            .project_authority
            .allowed_actions
            .intersection(&binding.environment_authority.allowed_actions)
            .copied()
            .collect(),
    };
    let authority = restrict_authority(
        &authority_ceiling,
        &definition.requirements.execution_restrictions,
    );
    let requirements = RequirementsRevision {
        revision: 1,
        base_revision: None,
        requirements: definition.requirements.clone(),
        parameters: parameters.clone(),
        authority: authority.clone(),
        status: RequirementsRevisionStatus::Active,
        requested_by: "initial_binding".into(),
        prior_actions: Vec::new(),
        reason: None,
    };
    Ok(BoundOptimizationDefinition {
        run_id: binding.run_id,
        definition: definition.clone(),
        context: binding.context,
        parameters,
        authority,
        authority_ceiling,
        requirements,
    })
}

/// Render resolved settings for preview/control without resolving or exposing
/// secret references. Unlabelled literals are explicitly non-secret data.
pub fn inspect_binding(binding: &BoundOptimizationDefinition) -> serde_json::Value {
    serde_json::json!({
        "run_id": binding.run_id,
        "definition_id": binding.definition.id,
        "definition_revision": binding.definition.revision,
        "context": binding.context,
        "requirements": binding.requirements.requirements,
        "requirements_revision": binding.requirements.revision,
        "parameters": inspect_parameters(&binding.requirements.parameters),
        "allowed_actions": binding.requirements.authority.allowed_actions,
    })
}

/// Render literal settings and redacted secret-reference placeholders.
pub fn inspect_parameters(parameters: &BTreeMap<String, ParameterValue>) -> serde_json::Value {
    parameters
        .iter()
        .map(|(key, value)| {
            let rendered = match value {
                ParameterValue::Literal { value } => value.clone(),
                ParameterValue::SecretReference { .. } => serde_json::json!("[secret reference]"),
            };
            (key.clone(), rendered)
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Check a host action independently of acceptance or ordinary parameter values.
pub fn authorize_action(
    authority: &ExecutionAuthority,
    action: ExecutionAction,
) -> Result<(), OptimizationError> {
    if !authority.allowed_actions.contains(&action) {
        return Err(OptimizationError::Unauthorized(format!(
            "action {action:?} is not allowed"
        )));
    }
    Ok(())
}

pub(super) fn ensure_enforceable(
    restrictions: &ExecutionRestrictions,
    capabilities: &EnforcementCapabilities,
) -> Result<(), OptimizationError> {
    let unsupported: Vec<_> = restrictions
        .denied_actions
        .difference(&capabilities.denied_actions)
        .collect();
    if !unsupported.is_empty() {
        return Err(OptimizationError::UnsupportedRestriction(format!(
            "host cannot prohibit {unsupported:?}"
        )));
    }
    if !restrictions.protected_paths.is_empty() && !capabilities.protected_paths {
        return Err(OptimizationError::UnsupportedRestriction(
            "host cannot protect evaluator/input paths from candidate writes".into(),
        ));
    }
    Ok(())
}

pub(super) fn restrict_authority(
    ceiling: &ExecutionAuthority,
    restrictions: &ExecutionRestrictions,
) -> ExecutionAuthority {
    ExecutionAuthority {
        allowed_actions: ceiling
            .allowed_actions
            .difference(&restrictions.denied_actions)
            .copied()
            .collect(),
    }
}

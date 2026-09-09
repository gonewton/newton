use super::{ExecutionAuthority, OptimizationRequirements, ParameterValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Observable lifecycle of a local requirements update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequirementsRevisionStatus {
    /// Validated request, not yet acknowledged or authoritative.
    Pending,
    /// Acknowledged at an enforceable execution boundary.
    Active,
    /// Rejected request retained for diagnosis.
    Rejected,
    /// Previously active policy retained in history.
    Superseded,
}

/// Versioned requirements, resolved settings, authority, and update diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementsRevision {
    /// Monotonically increasing acknowledged revision; initial revision is one.
    pub revision: u64,
    /// Active revision against which this update was submitted.
    pub base_revision: Option<u64>,
    /// Complete requirements snapshot, not a reference to a mutable source file.
    pub requirements: OptimizationRequirements,
    /// Resolved values including visible subsequent overrides.
    pub parameters: BTreeMap<String, ParameterValue>,
    /// Effective actions under the immutable project/environment ceiling.
    pub authority: ExecutionAuthority,
    /// Pending, active, rejected, or historical policy.
    pub status: RequirementsRevisionStatus,
    /// Identity supplied by the authenticated local control boundary.
    pub requested_by: String,
    /// Relevant actions already taken, which a new restriction cannot undo.
    #[serde(default)]
    pub prior_actions: Vec<String>,
    /// Explanation for pending/rejected activation or acknowledged limitations.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Declarative local update request; permission comes from the receiving host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementsUpdate {
    /// Compare-and-swap guard against stale concurrent requests.
    pub base_revision: u64,
    /// Full replacement requirements, validated before acknowledgment.
    pub requirements: OptimizationRequirements,
    /// Ordinary setting overrides; these cannot grant actions.
    #[serde(default)]
    pub parameter_overrides: BTreeMap<String, ParameterValue>,
}

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Current declarative Optimization Definition schema version.
pub const OPTIMIZATION_DEFINITION_VERSION: u32 = 1;

/// Reusable process declaration. Loading this data never executes its references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationDefinition {
    /// Serialization version; currently exactly `1`.
    pub schema_version: u32,
    /// Stable, author-controlled definition identity.
    pub id: String,
    /// Immutable author-controlled version of this definition.
    pub revision: String,
    /// Strategy identifier; the generic contract does not require Findings.
    pub strategy: String,
    /// Role to workflow path, interpreted by the selected strategy.
    pub workflows: BTreeMap<String, String>,
    /// Relative local helper/input files copied with workflows into a run snapshot.
    /// Include transitive local imports; external executables are host prerequisites.
    #[serde(default)]
    pub assets: Vec<String>,
    /// Initial policy copied into the first Requirements Revision.
    pub requirements: OptimizationRequirements,
    /// Ordinary values overridden by project values, then explicit run values.
    #[serde(default)]
    pub defaults: BTreeMap<String, ParameterValue>,
}

/// A non-secret literal or a reference resolved only by the authorized host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterValue {
    /// Non-sensitive declarative JSON data; never executable configuration.
    Literal { value: serde_json::Value },
    /// Secret provider reference; raw secret bytes are not stored in a definition.
    SecretReference { reference: String },
}

/// Requirements changed only through an acknowledged Requirements Revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationRequirements {
    /// Primary numeric/Grade objective or independent threshold objectives.
    pub objective: ObjectiveMode,
    /// Named, immutable evaluator references used by measurements and checks.
    pub evaluators: BTreeMap<String, EvaluatorReference>,
    /// Explicit rule for distinguishing improvement from noise.
    pub comparison: ComparisonPolicy,
    /// Conditions all accepted results must satisfy; initially failing is allowed.
    #[serde(default)]
    pub acceptance_constraints: Vec<AcceptanceConstraint>,
    /// Action restrictions applied independently of measurement and settings.
    pub execution_restrictions: ExecutionRestrictions,
    /// Finite elapsed-time and work/evaluation budgets.
    pub resource_limits: ResourceLimits,
    /// Destination checks, distinct from acceptable incremental improvement.
    pub completion: Vec<CompletionCriterion>,
}

/// How objective progress is measured; objectives are never implicitly weighted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectiveMode {
    /// Improve a single measurement subject to independent constraints.
    Primary { objective: ObjectiveSpec },
    /// Require every Grade target and preserve per-objective stopping guards.
    Thresholds { objectives: Vec<ThresholdObjective> },
}

/// A named measurement made by a particular evaluator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectiveSpec {
    /// Key under which measurements are emitted.
    pub id: String,
    /// Key in the requirements' evaluator map.
    pub evaluator: String,
    /// Native units/direction or rubric Grade semantics.
    pub measurement: MeasurementKind,
}

/// Native numeric values retain their units; rubric Grades remain 0–100.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasurementKind {
    /// Unscaled, finite numeric measurement with an explicit direction.
    Numeric { unit: String, direction: Direction },
    /// A rubric score in 0–100, where larger is better.
    Grade { dimension: String },
}

/// Direction of improvement for a native numeric objective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Smaller measurements are preferable.
    Minimize,
    /// Larger measurements are preferable.
    Maximize,
}

/// Independent Grade target and stopping guards for the threshold strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThresholdObjective {
    /// A Grade objective; native numeric values use primary mode.
    pub objective: ObjectiveSpec,
    /// Required score, inclusive, in 0–100.
    pub target: f64,
    /// Maximum tolerated score decrease from the baseline.
    pub regression_delta: f64,
    /// Consecutive non-improving cycles before the strategy stops.
    pub no_progress_cycles: u64,
}

/// Immutable evaluator identity, including protected inputs/rubric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluatorReference {
    /// Executable workflow role/path reference; loading does not authorize it.
    pub workflow: String,
    /// Content digest or immutable revision covering evaluator and authoritative inputs.
    pub revision: String,
}

/// Comparison policy applied to measurements in the objective's own units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonPolicy {
    /// One stable sample; an exact tie preserves the existing result.
    Exact,
    /// Repeated samples with conservative, non-overlapping observed ranges.
    /// This is not a confidence interval or a statistical significance claim.
    Repeated {
        /// Exact number of observations required per objective and candidate.
        samples: u32,
        /// Required gap between the worst candidate and best accepted sample.
        min_improvement: f64,
    },
}

/// An acceptance condition and whether its judgment requires a human.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceConstraint {
    /// Unique check identity.
    pub id: String,
    /// Key in the requirements' evaluator map.
    pub evaluator: String,
    /// Human criteria require evidence identifying the responsible reviewer.
    #[serde(default)]
    pub human_judged: bool,
}

/// A destination requirement; every declared criterion must be satisfied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionCriterion {
    /// Native/Grade target in the objective's own units and direction.
    ObjectiveTarget { objective: String, target: f64 },
    /// Explicit evaluator check, independent of the strategy's stop signal.
    Check { constraint: AcceptanceConstraint },
}

/// Actions whose permission must be enforced by the execution host.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAction {
    /// Dispatch a third-party coding agent.
    Agent,
    /// Execute an external command.
    Command,
    /// Access external networks.
    Network,
    /// Create local candidate commits.
    Commit,
    /// Publish an explicitly authorized draft pull request.
    DraftPullRequest,
    /// Publish non-draft work or push changes remotely.
    Publish,
    /// Integrate into an accepted branch.
    Merge,
    /// Deploy an artifact.
    Deploy,
}

/// Hard restrictions, never preferences compensated for by a better score.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRestrictions {
    /// Forbidden actions, enforced across agents, commands, and nested workflows.
    #[serde(default)]
    pub denied_actions: BTreeSet<ExecutionAction>,
    /// Evaluator/rubric/input paths that the host must protect from candidate writes.
    #[serde(default)]
    pub protected_paths: BTreeSet<String>,
}

/// Independent project/environment permissions; ordinary overrides cannot add grants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAuthority {
    /// Explicitly allowed actions. The empty set grants no execution permissions.
    pub allowed_actions: BTreeSet<ExecutionAction>,
}

/// Finite budgets; no hard monetary/token guarantees are implied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    /// Whole-run wall-clock budget, including evaluator time.
    pub elapsed_seconds: u64,
    /// Maximum cycles, including unsuccessful attempts.
    pub max_cycles: u64,
    /// Maximum work dispatches, including driver-managed retries. Hosts must
    /// meter internal task retries against this limit or reject their use.
    pub max_work: u64,
    /// Maximum evaluator dispatches, including repeats and driver-managed retries.
    /// Hosts must meter internal task retries against this limit or reject them.
    pub max_evaluations: u64,
}

/// Repository or other local context; no portfolio hierarchy is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationContext {
    /// Stable identity chosen by the caller.
    pub id: String,
    /// Local execution root, resolved and validated by the host.
    pub root: String,
}

/// Immutable run binding; persist this snapshot before starting any work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundOptimizationDefinition {
    /// Owning Optimize Run identity.
    pub run_id: String,
    /// Full source snapshot; reloading its source must not alter a resumed run.
    pub definition: OptimizationDefinition,
    /// Effective execution context.
    pub context: OptimizationContext,
    /// Default → project → explicit run values, retaining secret references only.
    pub parameters: BTreeMap<String, ParameterValue>,
    /// Intersection of project/environment authority with restrictions removed.
    pub authority: ExecutionAuthority,
    /// Immutable project/environment intersection, before definition restrictions.
    pub authority_ceiling: ExecutionAuthority,
    /// The initial active Requirements Revision, starting at one.
    pub requirements: super::RequirementsRevision,
}

use super::{MeasurementKind, OptimizationRequirements};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable candidate identity. Artifact identifiers must identify immutable states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// Unique candidate identity within an Optimize Run.
    pub id: String,
    /// Content-addressed state, such as a Git tree/commit or artifact digest.
    pub artifact_id: String,
    /// Accepted/integration base against which the candidate was produced.
    pub base_artifact_id: String,
    /// Requirements active when development began; evaluation may use a newer revision.
    pub created_under_revision: u64,
}

/// A measurement and an execution failure are disjoint outcomes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectiveMeasurement {
    /// Evaluator successfully produced observations in the declared units.
    Produced {
        /// Native/Grade shape must match the declared objective exactly.
        measurement: MeasurementKind,
        /// Exact or repeated raw samples; never silently average missing samples.
        samples: Vec<f64>,
    },
    /// Evaluation failed operationally; not a score, tie, or negative observation.
    Error { message: String },
}

/// Three-valued condition evidence: unavailable evidence never passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Evidence establishes the condition.
    Satisfied,
    /// Evidence establishes a violation.
    Violated,
    /// Missing, unavailable, failed, or inconclusive evidence.
    Unknown,
}

/// Evidence for an explicitly named evaluator check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    /// Evaluator key from the active requirements.
    pub evaluator: String,
    /// A failed/unavailable check must be `unknown`, not `satisfied`.
    pub status: CheckStatus,
    /// Diagnostic or proof artifact references.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Required identity for human-judged criteria.
    #[serde(default)]
    pub judged_by: Option<String>,
}

/// Fully correlated evaluation of one immutable candidate under one revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateEvaluation {
    /// Unique evidence record identity.
    pub id: String,
    /// Owning Optimize Run, not merely repository identity.
    pub run_id: String,
    /// Owning Cycle number.
    pub cycle: u64,
    /// Identity of the exact evaluated candidate.
    pub candidate_id: String,
    /// Immutable artifact evaluated.
    pub artifact_id: String,
    /// Integration base evaluated; changed integration requires revalidation.
    pub base_artifact_id: String,
    /// Latest acknowledged revision used by this evaluator invocation.
    pub requirements_revision: u64,
    /// Immutable evaluator/rubric/input versions actually used.
    pub evaluator_revisions: BTreeMap<String, String>,
    /// Objective identifier to raw measurement or operational failure.
    pub measurements: BTreeMap<String, ObjectiveMeasurement>,
    /// Acceptance check identifier to evidence.
    pub constraints: BTreeMap<String, CheckEvidence>,
    /// Separate completion-check identifier to evidence.
    pub completion_checks: BTreeMap<String, CheckEvidence>,
}

/// Retained accepted state and the evidence that qualified it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptedResult {
    /// Stable artifact identity; the host must preserve it during exploration.
    pub candidate: Candidate,
    /// Successful qualifying evidence, not merely development/test success.
    pub evaluation: CandidateEvaluation,
}

/// Individual objective comparison; initial qualification never claims improvement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOutcome {
    /// There is no currently qualifying accepted baseline.
    Initial,
    /// Candidate satisfies the declared improvement rule.
    Better,
    /// Candidate is demonstrably worse under the comparison policy.
    Worse,
    /// Stable measurements are exactly equal.
    Tie,
    /// Evidence is insufficient or noisy sample ranges overlap.
    Inconclusive,
}

/// Result of evaluation; no disposition grants merge, publication, or deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDisposition {
    /// First qualifying result, with no invented improvement over a missing baseline.
    InitialQualification,
    /// Qualifying incremental improvement over a current accepted result.
    Improvement,
    /// Objective regression or an explicit acceptance violation.
    Rejected,
    /// Exact tie; existing accepted state is retained.
    Unchanged,
    /// Missing, stale, failed, or inconclusive evidence; existing state is retained.
    Inconclusive,
}

/// Pure policy decision returned to the driver for durable recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateDecision {
    /// Candidate that was considered.
    pub candidate_id: String,
    /// Active revision used by this decision.
    pub requirements_revision: u64,
    /// Whether this evaluation establishes acceptance.
    pub disposition: CandidateDisposition,
    /// Per-objective comparisons, never an aggregate weighted score.
    pub comparisons: BTreeMap<String, ComparisonOutcome>,
    /// Qualifying replacement only; `None` means do not replace prior state.
    pub accepted_result: Option<AcceptedResult>,
    /// User-visible diagnostic explanations.
    pub reasons: Vec<String>,
}

/// Completion is a separate claim from either acceptance or stopping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompletionAssessment {
    /// All criteria satisfied, at least one violated, or evidence unknown.
    pub status: CheckStatus,
    /// Criterion key to individual result, suitable for a local report.
    pub criteria: BTreeMap<String, CheckStatus>,
    /// Missing/stale evidence and failure details.
    pub reasons: Vec<String>,
}

/// Why execution stopped, independently of completion and accepted results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationStopReason {
    /// The active completion criteria were verified.
    Completed,
    /// A requested one-cycle invocation finished; completion is assessed separately.
    CycleComplete,
    /// Finite work, evaluation, cycle, or elapsed-time budget was consumed.
    ResourceLimit,
    /// Strategy found no remaining actionable work; not proof of completion.
    NoActionableWork,
    /// The mode-specific consecutive no-progress guard fired.
    NoProgress,
    /// A mode-specific regression guard fired.
    Regression,
    /// Grading, planning, execution, persistence, or other operation failed.
    OperationalFailure,
    /// User cancellation; never synthesized completion.
    Cancelled,
    /// Ambiguous side effects, blocked work, or enforcement needs a human.
    NeedsIntervention,
}

/// Counters actually available to the local runtime; retries/repeats must count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceUsage {
    /// Elapsed whole-run seconds, including evaluation and waiting.
    pub elapsed_seconds: u64,
    /// Cycles started.
    pub cycles: u64,
    /// Work dispatches started, including retries.
    pub work: u64,
    /// Evaluator dispatches started, including repeats/retries.
    pub evaluations: u64,
}

/// Honest final report; a resource stop is not evidence of impossibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationOutcome {
    /// Owning Optimize Run identity.
    pub run_id: String,
    /// Latest acknowledged requirements revision.
    pub requirements_revision: u64,
    /// Policy in force at the end of the run.
    pub requirements: OptimizationRequirements,
    /// Operational stopping reason, not a proxy for completion.
    pub stop_reason: OptimizationStopReason,
    /// Independent completion assessment.
    pub completion: CompletionAssessment,
    /// Best retained result only if it still qualifies under current requirements.
    pub accepted_result: Option<AcceptedResult>,
    /// True when no currently qualifying result exists, not a feasibility claim.
    pub no_acceptable_result_found: bool,
    /// Retained identities whose evidence is not current; never silently rolled back.
    pub historical_result_ids: Vec<String>,
    /// Visible blocked work, even in automatic approval mode.
    pub blocked_work: Vec<String>,
    /// Known resource usage without invented monetary/token accounting.
    pub usage: ResourceUsage,
    /// Failures, stale evidence, and recoverability references.
    pub diagnostics: Vec<String>,
}

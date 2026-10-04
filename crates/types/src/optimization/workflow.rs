use super::{
    AcceptedResult, Candidate, CandidateDecision, CandidateEvaluation, OptimizationOutcome,
};
use serde::{Deserialize, Serialize};

/// Optional assessment details preserved exactly with an evaluator output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessmentDetails {
    /// Assessment-local identity.
    pub id: String,
    /// Human-readable evaluator conclusion.
    #[serde(default)]
    pub summary: String,
    /// Optional suggestions used by observation-driven strategies.
    #[serde(default)]
    pub observations: Vec<AssessmentObservation>,
    /// Optional declaration that omission has a defined meaning.
    #[serde(default)]
    pub coverage: Option<AssessmentCoverage>,
}

/// One immutable, assessment-local observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessmentObservation {
    /// Identity unique within the containing assessment.
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub suggested_action: String,
    /// Strategy-specific priority; severity is not universally required.
    #[serde(default)]
    pub priority: Option<f64>,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Optional best-effort links to observations in earlier assessments.
    #[serde(default)]
    pub links: Vec<ObservationLink>,
    /// Optional evaluator-backed conclusion about earlier work.
    #[serde(default)]
    pub resolution: Option<ObservationResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationLink {
    pub assessment_id: String,
    pub observation_id: String,
    pub relationship: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationResolution {
    pub status: ObservationResolutionStatus,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationResolutionStatus {
    Resolved,
    Unresolved,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessmentCoverage {
    pub scope: String,
    pub complete: bool,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// One evaluator invocation. Repeated samples are stored separately in history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluationOutput {
    pub candidate: Candidate,
    pub evaluation: CandidateEvaluation,
    #[serde(default)]
    pub assessment: Option<AssessmentDetails>,
}

/// Reference to an observation selected from a specific immutable assessment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectedObservation {
    pub assessment_id: String,
    pub observation_id: String,
    pub rationale: String,
}

/// A known-safe unsuccessful attempt. Uncertain effects are operational failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttemptFailure {
    pub reason: String,
    /// Evidence that no ambiguous external effect needs reconciliation.
    pub evidence: Vec<String>,
}

/// Output of the proposal workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalOutput {
    /// Proposal already produced an immutable candidate; execution is skipped.
    Candidate {
        proposal_id: String,
        candidate: Candidate,
        rationale: String,
        #[serde(default)]
        selected_observations: Vec<SelectedObservation>,
        #[serde(default)]
        plan: Option<serde_json::Value>,
    },
    /// Proposal requires the configured execute workflow.
    Execute {
        proposal_id: String,
        rationale: String,
        attempt: serde_json::Value,
        #[serde(default)]
        selected_observations: Vec<SelectedObservation>,
        #[serde(default)]
        plan: Option<serde_json::Value>,
    },
    /// Strategy has no useful next action.
    None { reason: String },
    /// Proposal failed safely and can count as an unsuccessful Cycle.
    Failed {
        proposal_id: String,
        failure: AttemptFailure,
    },
}

/// Output of an optional execute workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionOutput {
    Candidate { candidate: Candidate },
    Failed { failure: AttemptFailure },
}

/// Immutable completed-Cycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CycleStatus {
    Accepted,
    Rejected,
    Inconclusive,
    FailedSafely,
    NoActionableWork,
    Completed,
    ThresholdStop,
    OperationalFailure,
    NeedsIntervention,
}

/// Immutable audit record committed before the mutable checkpoint advances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationCycleRecord {
    pub schema_version: u32,
    pub run_id: String,
    pub cycle: u64,
    pub requirements_revision: u64,
    pub started_at: String,
    pub completed_at: String,
    /// Original evaluator invocations for the incumbent/baseline.
    pub baseline_evaluations: Vec<EvaluationOutput>,
    #[serde(default)]
    pub proposal: Option<ProposalOutput>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub execution: Option<ExecutionOutput>,
    /// Original evaluator invocations for the proposed candidate.
    #[serde(default)]
    pub candidate_evaluations: Vec<EvaluationOutput>,
    #[serde(default)]
    pub decision: Option<CandidateDecision>,
    #[serde(default)]
    pub retained_result: Option<AcceptedResult>,
    pub status: CycleStatus,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

/// Immutable run metadata. Mutable progress belongs in `current.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationRunRecord {
    pub schema_version: u32,
    pub run_id: String,
    pub created_at: String,
    pub binding: serde_json::Value,
}

/// Read-only snapshot derived from per-run JSON files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationRunSnapshot {
    pub run: OptimizationRunRecord,
    pub current: serde_json::Value,
    pub cycles: Vec<OptimizationCycleRecord>,
    #[serde(default)]
    pub outcome: Option<OptimizationOutcome>,
}

/// Portable before/after report derived entirely from authoritative JSON history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptimizationReport {
    pub schema_version: u32,
    pub run_id: String,
    /// Original evaluator invocations from the first Cycle baseline.
    pub before: Vec<EvaluationOutput>,
    /// Best retained result and its qualifying evidence, when one exists.
    #[serde(default)]
    pub after: Option<AcceptedResult>,
    /// Completed attempts, including rejected and safely failed work.
    pub cycles: Vec<OptimizationCycleRecord>,
    pub outcome: OptimizationOutcome,
}

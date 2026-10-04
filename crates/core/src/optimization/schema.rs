use newton_types::optimization::{
    CandidateEvaluation, EvaluationOutput, ExecutionOutput, OptimizationCycleRecord,
    OptimizationDefinition, OptimizationReport, ProposalOutput, RequirementsUpdate,
};

/// JSON Schema generated from the exact Optimization Definition wire types.
/// Semantic rules, evaluator trust, and host capabilities still require binding.
pub fn optimization_definition_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(OptimizationDefinition))
        .expect("generated definition schema is JSON serializable")
}

/// JSON Schema for candidate evidence, including disjoint produced/error outcomes.
/// Identity and finite/unit/sample checks remain part of runtime validation.
pub fn candidate_evaluation_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(CandidateEvaluation))
        .expect("generated evaluation schema is JSON serializable")
}

/// JSON Schema for local revision requests; authorization is host-supplied.
pub fn requirements_update_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(RequirementsUpdate))
        .expect("generated requirements-update schema is JSON serializable")
}

/// JSON Schema for one evaluator invocation, including optional assessment details.
pub fn evaluation_output_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(EvaluationOutput))
        .expect("generated evaluator-output schema is JSON serializable")
}

/// JSON Schema for the proposal workflow's domain-neutral output.
pub fn proposal_output_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(ProposalOutput))
        .expect("generated proposal-output schema is JSON serializable")
}

/// JSON Schema for the optional execution workflow's output.
pub fn execution_output_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(ExecutionOutput))
        .expect("generated execution-output schema is JSON serializable")
}

/// JSON Schema for one immutable completed optimization Cycle.
pub fn optimization_cycle_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(OptimizationCycleRecord))
        .expect("generated Cycle schema is JSON serializable")
}

/// JSON Schema for the portable before/after run report.
pub fn optimization_report_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(OptimizationReport))
        .expect("generated optimization-report schema is JSON serializable")
}

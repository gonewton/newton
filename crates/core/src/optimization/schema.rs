use newton_types::optimization::{CandidateEvaluation, OptimizationDefinition, RequirementsUpdate};

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

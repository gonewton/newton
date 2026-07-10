# AUTO-GENERATED — do not edit by hand.
# Regenerate with: bash codegen/generate.sh
OUTPUT_SCHEMAS: dict[str, list[str]] = {
    "AgentOperator": ['exit_code', 'signal', 'stdout_artifact', 'stop_reason'],
    "AssertCompletedOperator": ['all_succeeded'],
    "ChangeRequestOperator": ['change_request_id', 'decision'],
    "CommandOperator": ['duration_ms', 'exit_code', 'stderr', 'stdout', 'success'],
    "GraderAgentOperator": ['assessment', 'counts', 'overall_score', 'score_by_dimension', 'verdict'],
    "GraderCommandOperator": ['assessment', 'counts', 'overall_score', 'score_by_dimension', 'verdict'],
    "HumanApprovalOperator": ['approved', 'outcome'],
    "HumanDecisionOperator": ['choice'],
    "NoOpOperator": ['status'],
    "ReconcileOperator": ['created', 'refreshed', 'reopened', 'resolved'],
    "SetContextOperator": ['applied', 'patch'],
    "barrier": ['barrier_passed', 'expected_tasks', 'message'],
}

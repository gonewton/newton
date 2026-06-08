// AUTO-GENERATED — do not edit by hand.
// Regenerate with: bash codegen/generate.sh
export const OUTPUT_SCHEMAS: Record<string, string[]> = {
  AgentOperator: ["exit_code", "signal", "stdout_artifact"],
  AssertCompletedOperator: ["all_succeeded"],
  CommandOperator: ["duration_ms", "exit_code", "stderr", "stdout", "success"],
  HumanApprovalOperator: ["approved", "outcome"],
  HumanDecisionOperator: ["choice"],
  NoOpOperator: ["status"],
  SetContextOperator: ["applied", "patch"],
  barrier: ["barrier_passed", "expected_tasks", "message"],
};

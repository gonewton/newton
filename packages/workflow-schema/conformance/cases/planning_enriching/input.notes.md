# planning_enriching workflow

Spec enrichment workflow invoked via WorkflowOperator from planner.yaml, or standalone.

Key characteristics:
- Two operators: AgentOperator and CommandOperator
- Entry task: enrich_spec
- Has a dead/unreachable task-1 at the end (compiler should flag as warning)
- Conditional branching on check_gaps stdout (has_gaps vs no_gaps)
- Uses injected ambient vars: develop_primary_engine, develop_primary_model
- Uses triggers.output_path and triggers.prompt
- max_workflow_iterations: 15, max_task_iterations: 3
- Simplest real workflow — good first round-trip target

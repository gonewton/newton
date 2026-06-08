# develop workflow

Full software development cycle workflow: git operations, spec implementation, testing, PR creation.

Key characteristics:
- Entry task: ensure_clean_main
- Complex graph with multiple loops (overlapping, non-nested cycles)
- Uses GhOperator: project_resolve_board, project_item_set_status, pr_create, pr_view, pr_approve
- Uses AgentOperator with signals (complete, valid, invalid)
- Injected vars: develop_primary_engine, develop_primary_model, develop_secondary_engine, develop_secondary_model
- Uses typed refs: tasks.X.output.stdout, tasks.X.output.signal, tasks.X.output.pr_number, tasks.X.output.state
- max_workflow_iterations: 500, max_task_iterations: 15000
- artifact_storage configured
- Terminal tasks: fail_validation_oversized (failure), no_changes_done (success), success (success)

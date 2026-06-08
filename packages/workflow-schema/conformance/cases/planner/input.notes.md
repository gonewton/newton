# planner workflow

Orchestrator workflow: resolves board, enriches spec via sub-workflow, injects frontmatter, publishes to GitHub.

Key characteristics:
- Entry task: resolve_board_ids
- Uses WorkflowOperator (sub_workflow) via invoke_enricher task
- Uses GhOperator: project_resolve_board, project_item_set_status
- Uses CommandOperator with shell scripts
- Context preamble with large prompt template
- No loops — simple linear graph with one branch
- max_workflow_iterations: 15
- Terminal task: success

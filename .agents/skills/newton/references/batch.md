# newton batch

## Purpose

Headless **queue runner**: for each markdown plan in `.newton/plan/<project_id>/todo/`, copy it into the per-task layout under `project_root/.newton/tasks/<task_id>/` and execute the **configured workflow YAML** the same way as `newton run`, then move the plan to `completed/` or `failed/`.

## Required input

- **`PROJECT_ID`**: Selects `.newton/configs/<project_id>.conf` under the discovered Newton workspace root.

## Options

- `--workspace <PATH>`: Newton workspace root containing `.newton` (default: walk up from the current directory until `.newton` is found).
- `--once`: Process one todo plan and exit.
- `--sleep <SECONDS>`: Poll interval when the queue is empty (default: 60).

## Configuration

See [configuration.md](configuration.md). Batch requires `project_root` and `workflow_file` (or `workflow_path` as an alias).

Example:

```bash
project_root=/path/to/repo
workflow_file=.newton/workflows/develop.yaml
```

## Plan queue

- Plans start in `.newton/plan/<project_id>/todo/`.
- State and workflow artifacts go under `project_root/.newton/tasks/<task_id>/`.
- Successful runs move the plan to `completed/`; failures move it to `failed/`.

## Example

```bash
newton batch my-project --workspace ~/my-workspace --once
```

## Notes

- Batch injects a manual trigger with `input_file` and `workspace` pointing at the plan copy and project root.
- Hooks, agents, and branching belong in workflow YAML and operators, not in `.conf` classic-loop keys.

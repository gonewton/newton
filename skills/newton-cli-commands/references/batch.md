# newton batch

## Purpose

Headless **queue runner**: for each markdown plan in `.newton/plan/<project_id>/todo/`, copy it into the task layout and execute the **configured workflow YAML** the same way as `newton run`, then move the plan to `completed/` or `failed/`.

## Required input

- **`PROJECT_ID`**: Selects `.newton/configs/<project_id>.conf` under the discovered workspace.

## Options

- `--workspace <PATH>`: Newton workspace root containing `.newton` (default: walk up from current directory until `.newton` is found).
- `--once`: Process one todo plan and exit.
- `--sleep <SECONDS>`: Poll interval when the queue is empty (default: 60).

## Configuration (`.newton/configs/<project_id>.conf`)

Key/value lines (comments with `#` allowed). Required keys:

- **`project_root`**: Directory that contains a `.newton` directory (absolute or relative to the Newton workspace root).
- **`workflow_file`**: Path to the workflow YAML, relative to `project_root` or to the workspace root (see loader resolution in code).

Example:

```
project_root=.
workflow_file=newton/workflows/planner.yaml
```

## Plan queue

- Plans start in `.newton/plan/<project_id>/todo/`.
- Each run uses `.newton/tasks/<task_id>/` under the project for state and artifacts.
- Successful runs move the plan to `completed/`; failures move it to `failed/`.

## Example

```bash
newton batch my-project --workspace ~/my-workspace --once
```

## Notes

- Batch builds a manual trigger payload with `input_file` and `workspace` pointing at the plan and project root.
- For flags and behavior of the workflow itself, see [run.md](run.md) and `newton run --help`.

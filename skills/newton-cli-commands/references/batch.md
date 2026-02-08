# newton batch

## Purpose
Process plan files stored in `.newton/plan/<project_id>` using the Newton CLI, driving `newton run` for each todo entry and advancing the queue when runs succeed.

## Required Input
- `project_id`: Matches a file at `.newton/configs/<project_id>.conf`.
- Workspace root containing `.newton` (automatically discovered from the current working directory unless you override it with `--workspace`).

## Important Flags
- `--workspace <PATH>`: Path to the workspace root if you are not already inside one.
- `--once`: Process only the first todo item and exit (no daemon loop).
- `--sleep <SECONDS>`: Wait between polls when the queue is empty (default 60).

## Batch Configuration
`.newton/configs/<project_id>.conf` must provide:

```
project_root = ./project-directory
coding_agent = <agent-name>
coding_model = <model-spec>
```

Relative `project_root` values are resolved against the workspace root, and each project must contain its own `.newton` directory.

## Hooks
Add a `[hooks]` table to the target workspace or project `newton.toml`:

```toml
[hooks]
before_run = "git checkout main"
after_run = "git checkout $NEWTON_RESULT"
```

Hook commands execute via `sh -c "<value>"` using the project root as the working directory. `before_run` runs before the orchestrator and sees `NEWTON_GOAL_FILE`, `NEWTON_PROJECT_ID`, and `NEWTON_TASK_ID` (when invoked from batch). `after_run` runs on success and failure, receives `NEWTON_RESULT=success|failure`, and also inherits `NEWTON_EXECUTION_ID` when available.

Batch sets `CODING_AGENT`, `CODING_AGENT_MODEL`, `NEWTON_EXECUTOR_CODING_AGENT`, and `NEWTON_EXECUTOR_CODING_AGENT_MODEL` before invoking `newton run` so the evaluator/advisor/executor configuration honors the values declared in the `.conf`.

## Example

```bash
newton batch my-project --workspace ~/my-workspace --once
```

Newton copies the first file from `.newton/plan/my-project/todo` into `project_root/.newton/tasks/<task_id>/input/spec.md`, runs the usual optimization loop with that goal file, and moves the plan to `completed/` after a successful run.

# newton workflow run

## Purpose

Execute a **workflow graph** from a YAML file: tasks run according to dependencies, operators, checkpoints, goal gates, and completion policy.

> The top-level `newton run` was moved under the `workflow` group. Use `newton workflow run`.

## Arguments

- **`FILE`** (required positional): Path to the workflow YAML.
- **`INPUT_FILE`** (optional second positional): Stored in the trigger payload as `input_file` for workflows that expect a spec path.

## Options

- `--workspace <PATH>`: Workspace root (default: current directory). Checkpoints and artifacts resolve under this tree.
- `--trigger KEY=VALUE`: Merge into `triggers.payload` (repeatable, or comma-separated).
- `--context KEY=VALUE`: Merge into `workflow.context` at runtime (repeatable, or comma-separated).
- `--parameters-json <PATH>`: Load a JSON object as the base trigger payload before `--trigger` merges. Accepts a bare path or `@path`.
- `--parallel-limit N`: Override max concurrent tasks for this run.
- `--timeout N`: Wall-clock limit for this run, in seconds.
- `--verbose`: Print each task's stdout and stderr after it finishes.
- `--emit-completion-json`: Write the structured completion envelope to stdout as JSON.
- `--server <URL>`: Register this run with a Newton HTTP API (`newton serve`) for lifecycle updates.
- `--state-dir <PATH>`: Override the state root (checkpoints, artifacts, `backend.sqlite`).

## Examples

```bash
newton workflow run workflow.yaml --workspace .

newton workflow run workflow.yaml input/spec.md --workspace ./proj --trigger env=prod

newton workflow run ./workflows/ci.yaml --workspace . --verbose

newton workflow run workflow.yaml --server http://127.0.0.1:8080 --workspace .
```

## Notes

- Use `newton workflow validate`, `newton workflow lint`, and `newton workflow preview` on the same file to check or document behavior before running.
- Resume after interruption with `newton workflow resume --run-id <uuid>` once checkpoints exist for that execution.
- The classic evaluator-only or advisor-only loop without workflow YAML is not the current execution model; express control flow in workflow YAML instead.

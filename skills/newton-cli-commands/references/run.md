# newton run

## Purpose

Execute a **workflow graph** from a YAML file: tasks run according to dependencies, operators, completion policy, checkpoints, and goal gates.

## Arguments

- **`WORKFLOW`** (positional, optional if `--file` is set): Path to the workflow YAML.
- **`INPUT_FILE`** (optional second positional): Stored in the trigger payload as `input_file` for workflows that expect a spec path.

## Options

- `--file <PATH>`: Workflow path (overrides positional `WORKFLOW` when both are set).
- `--workspace <PATH>`: Workspace root (default: current directory). Checkpoints and artifacts resolve under this tree.
- `--arg KEY=VALUE`: Merge into `triggers.payload` (repeatable). Values may use `@path` to read file contents; use `@@` for a literal `@`.
- `--set KEY=VALUE`: Merge into workflow context at runtime (repeatable).
- `--trigger-json <PATH>`: Load a JSON object as the base trigger payload before `--arg` merges.
- `--parallel-limit N`: Override max concurrent tasks for this run.
- `--max-time-seconds N`: Wall-clock limit for this run.
- `--verbose`: Print each task's stdout/stderr after it finishes.
- `--server <URL>`: Register this run with a Newton HTTP API (`newton serve`) for lifecycle updates.

## Examples

```bash
newton run workflow.yaml --workspace .

newton run workflow.yaml input/spec.md --workspace ./proj --arg env=prod

newton run --file ./workflows/ci.yaml --workspace . --verbose

newton run workflow.yaml --server http://127.0.0.1:8080 --workspace .
```

## Notes

- Use `newton validate` / `newton lint` / `newton explain` on the same file to check or document behavior before running.
- Resume after interruption with `newton resume --execution-id <uuid>` once checkpoints exist for that execution.

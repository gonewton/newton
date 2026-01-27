# newton step

## Purpose
Executes exactly one iteration of the optimization loop, allowing manual control of the evaluation/advice/execution phases without running until convergence.

## Required Input
- `WORKSPACE`: Path where Newton artifacts are stored.

## Important Flags
- `--execution-id <ID>`: associate the step with a tracking identifier for auditing/log aggregation.

## Example Invocation
```bash
cargo run -- step ./workspace --execution-id exec_456
```

## Extra Tips
- Use `newton step --help` to display all optional arguments.
- `--execution-id` is optional but recommended so downstream tooling can correlate logs.
- Ensure the workspace already contains artifacts from a previous `run` or `step` to avoid missing-context failures.

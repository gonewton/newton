# newton status

## Purpose
Queries the persisted artifacts for a specific execution to show active phase, iteration counts, and blockers.

## Required Input
- `EXECUTION`: Execution identifier to inspect.

## Important Flags
- `--workspace <PATH>`: optional workspace location (defaults to current directory).

## Example Invocation
```bash
cargo run -- status exec_123 --workspace ./workspace
```

## Extra Tips
- `newton status --help` lists every option and default.
- Set `--workspace` when querying executions stored outside the current directory.
- Status reads artifacts only and will not mutate files, so it is safe to run repeatedly.

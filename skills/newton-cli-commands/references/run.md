# newton run

## Purpose
Runs the complete Newton optimization loop, repeatedly executing evaluator, advisor, and executor tools until limits defined in `RunArgs` are met.

## Required Input
- `WORKSPACE`: Path to the workspace directory containing Newton manifests.

## Important Flags
- `--max-iterations <N>`: stop after N iterations (default 10).
- `--max-time <SECONDS>`: hard wall-clock cap (default 300).
- `--evaluator-cmd/--advisor-cmd/--executor-cmd`: override tool binaries for strict mode.
- `--evaluator-status-file`, `--advisor-recommendations-file`, `--executor-log-file`: redirect artifact paths.
- `--tool-timeout-seconds` and per-tool `--*-timeout` overrides.

## Example Invocation
```bash
cargo run -- run ./workspace --max-iterations 5 --max-time 120
```

## Extra Tips
- `newton run --help` shows every available flag with default values.
- Strict-mode command overrides should point to executable binaries accessible from your PATH.
- When redirecting artifact files, pre-create parent directories to avoid runtime errors.

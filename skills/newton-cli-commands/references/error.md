# newton error

## Purpose
Helps diagnose failures for a specific execution by surfacing stack traces, tool crashes, and incompatible artifacts.

## Required Input
- `EXECUTION`: Execution identifier to debug.

## Important Flags
- `--verbose`: include stack traces, raw logs, and contextual artifacts.

## Example Invocation
```bash
cargo run -- error exec_999 --verbose
```

## Extra Tips
- Run `newton error --help` for the full list of options.
- Pair `--verbose` with log tailing tools if you want to stream additional context while debugging.
- Always capture the command output; it often includes direct pointers to failing artifacts.

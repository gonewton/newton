# Logging

Newton exposes a deterministic logging framework that adapts to the current command, workspace state,
and explicit configuration so that every tool invocation has reliable outputs even when the monitor TUI
is running.

## Execution contexts

- **`Tui`** (`newton monitor`) emits nothing to the terminal to avoid corrupting the Ratatui rendering.
  Logs are still written to file and are safe to tail from another console.
- **`LocalDev`** commands (`run`, `init`, `step`, `status`, `report`, `error`) write to stderr by default so
  developers see feedback while the file sink persists the same records.
- **`Batch`** commands (`batch`) run headless, so console output is disabled and only the file sink is active.
- **`RemoteAgent`** is enabled when `NEWTON_REMOTE_AGENT=1` is set and covers all commands except `monitor`.
  File logging remains enabled, while the console defaults to disabled unless overridden.

Context detection happens before any sinks are configured; logging layers query the parsed `Command`
and the remote override to decide which outputs remain active.

## Configuration precedence

1. **CLI flags** (reserved for future expansion) would have top priority.
2. **Environment variables** such as `RUST_LOG`, `NEWTON_REMOTE_AGENT`, and `OTEL_EXPORTER_OTLP_ENDPOINT`
   override any file-based defaults. `RUST_LOG` still controls the tracing level filter.
3. **`.newton/config/logging.toml`** in the current workspace (when known) supplies persisted overrides.
4. **Built-in defaults** cover the remaining parameters (`info` level, file sink enabled, console on stderr,
   no OpenTelemetry).

You can combine these knobs, e.g., run `RUST_LOG=debug newton run ./workspace` to raise the level while
keeping the consolidated file sink in `.newton/logs/newton.log`.

### Sample configuration

```toml
[logging]
log_dir = "logs"
default_level = "debug"
enable_file = true
console_output = "stderr"

[logging.opentelemetry]
enabled = true
endpoint = "https://otel.example.com/v1/traces"
service_name = "newton-agent"
```

Relative `log_dir` values are resolved against the workspace root. When the workspace is unknown, the default
path is `$HOME/.newton/logs/newton.log`. OpenTelemetry endpoints are validated as URLs, and missing schemes
produce a user-friendly error before the command starts.

## Troubleshooting

### Avoiding TUI corruption

`newton monitor` uses the `Tui` execution context, which forces the console sink to `none` so that Ratatui can
draw without interleaving logs. If you need to inspect logs, tail the workspace log file (`.newton/logs/newton.log`)
or set `NEWTON_REMOTE_AGENT=1` and run the same command outside the monitor session. Running `RUST_LOG=debug`
while monitoring still writes the events to the file sink without touching the terminal.

If you must see console output, run the corresponding `run`, `step`, or `batch` command instead of the monitor,
or temporarily exit the TUI and re-run the CLI with `NEWTON_REMOTE_AGENT=1` so that the remote context allows
console sinks again.

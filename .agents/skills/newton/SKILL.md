---
name: newton
description: Newton CLI for workflow YAML graphs (operators, checkpoints, goal gates), batch plan queues, ailoop human-in-the-loop via monitor, and HTTP APIs via serve. Use when running or resuming workflows, validating or linting workflow files, managing checkpoints or artifacts, configuring .newton/configs, or using `workflow validate`, `workflow lint`, `workflow preview`, `workflow graph`, resume, runs, checkpoint, artifact, webhook, monitor, or batch.
license: Apache-2.0
compatibility: Requires the newton binary on PATH. newton init requires aikit on PATH for templates. newton monitor requires a running ailoop server unless your workflow docs say otherwise.
---

# Newton

Newton is a **workflow-first** CLI: YAML workflow graphs with operators, checkpoints, artifacts, and goal gates. Classic evaluator, advisor, and executor-only loops are expressed inside workflows now, not as separate top-level commands. **Sub-workflows** are supported: a task can invoke another workflow file with `WorkflowOperator` (`workflow_path`, optional `context` and `triggers` merges), subject to workspace path rules and a maximum nesting depth.

## When to use

- Running or resuming workflows (including graphs that call nested workflows via `WorkflowOperator`), batch plan queues, or webhook-driven runs.
- Initializing a workspace (`newton init`) and editing `.newton/configs/*.conf`.
- Validating or explaining workflow YAML; cleaning checkpoints or artifacts.
- Operating `newton monitor` against ailoop, or `newton serve` for HTTP or WebSocket APIs.

## Installation

```bash
brew tap gonewton/cli
brew install newton

scoop bucket add gonewton https://github.com/gonewton/scoop-bucket
scoop install newton
```

Verify: `newton --help` and `newton --version`.

## Quick start

1. `newton --help` and `newton <command> --help` for flags.
2. `newton init [PATH]` to create `.newton/` and install the template via `aikit` (PATH defaults to the current directory).
3. `newton run <workflow.yaml> --workspace <root>` (optional second positional input file for trigger payload).

## CLI commands (source order)

These subcommands match the current CLI (confirm with `newton --help` on your build):

| Command | Role |
| --- | --- |
| `run` | Execute a workflow graph from YAML |
| `init` | Create `.newton/` and install the default template |
| `batch` | Process queued plans under `.newton/plan/<project_id>/` |
| `serve` | HTTP/WebSocket API for workflow state and streaming |
| `monitor` | Terminal UI for ailoop HIL channels |
| `workflow validate` | Validate workflow YAML before run |
| `workflow graph` | Emit Graphviz DOT for the workflow graph (`--format dot --output <PATH>`) |
| `workflow lint` | Best-practice checks on a workflow file |
| `workflow preview` | Human-readable description of workflow behavior |
| `resume` | Continue from a checkpoint (`--run-id`) |
| `runs` | `list` past runs / `show <RUN_ID>` task replay |
| `checkpoint` | `list` / `clean` checkpoint data |
| `artifact` | `clean` old execution artifacts |
| `webhook` | `serve` or `status` for webhook-triggered runs (`--workflow <PATH>`) |

For commands without a dedicated reference file below, use `newton <cmd> --help` as the source of truth for flags and examples.

There is **no** `step`, `status`, `report`, or `error` subcommand in current releases. Inspect runs via **checkpoints**, **resume**, **artifacts**, workflow logs, and `.newton/tasks/` under the project workspace. See [references/step.md](references/step.md) and related stubs for migration hints.

## Typical flows

1. **New workspace**: `newton init .` then set `workflow_file` in `.newton/configs/default.conf` when using batch; run workflows with `newton run path/to/workflow.yaml --workspace .`.
2. **Queue of plans**: Configure `.newton/configs/<project_id>.conf` with `project_root` and `workflow_file`; place plans in `.newton/plan/<project_id>/todo/`; run `newton batch <project_id>`.
3. **Live HIL**: Start [ailoop](https://github.com/goailoop/ailoop), point `.newton/configs/monitor.conf` at HTTP and WebSocket URLs (or pass `--ailoop-http` / `--ailoop-ws`), then `newton monitor`.
4. **API / dashboards**: `newton serve` exposes REST, WebSocket, and SSE endpoints for workflow instances and streams (see `newton serve --help` and the Newton repository `README.md` when updated).

## Usage notes

- `newton init` requires `aikit` on `PATH` and refuses to run if `.newton` already exists (remove it or pick another directory).
- `newton run` takes the workflow path as the required first positional argument; the legacy named flag is gone.
- `--server <URL>` on `newton run` registers the run with a Newton API instance started via `newton serve` for lifecycle notifications.
- Checkpoint and artifact layouts live under `.newton/` inside the workspace you pass with `--workspace` (or the discovered project root for batch).

## Quick reference

```bash
newton run workflow.yaml --workspace . --verbose
newton batch my-project --workspace ~/ws --once
newton workflow validate workflow.yaml
newton workflow lint workflow.yaml
newton workflow preview workflow.yaml
newton resume --run-id <uuid> --workspace .
newton monitor
```

## MCP Server Mode

Newton exposes every registered command as an MCP (Model Context Protocol) tool when the binary is invoked with the top-level `--mcp-serve` flag. This lets Cursor, Claude Desktop, and custom MCP agents drive Newton workflows over standard MCP without bespoke shims. The transport and tool derivation are owned by the upstream `cli-framework` crate (`mcp-server` feature); Newton wires the flag surface and emits a structured startup log line.

`--mcp-serve` is **a top-level mode**, not a subcommand argument. It short-circuits subcommand dispatch and is mutually exclusive with `newton serve` in a single process — run them in separate processes if you need both.

### MCP flags (Newton operator targets, spec §4.2)

| Flag | Default | Description |
| --- | --- | --- |
| `--mcp-serve` | off | Enable MCP server mode |
| `--mcp-host` | `127.0.0.1` | Bind address for the Streamable HTTP listener |
| `--mcp-port` | `8730` | Distinct from `newton serve` (8080) to avoid collision |
| `--mcp-path` | `/mcp` | HTTP path prefix for the MCP endpoint |

### Default-port caveat

The current upstream `cli-framework` clap definition prints `--mcp-port [default: 8080]` in `newton --help`, but Newton's argv layer rewrites unset values to `8730` before handing off to the framework — so the actual bind matches the table above. To avoid confusion, **always pass `--mcp-port` explicitly** until the upstream default is aligned (tracked at [cli-framework#29](https://github.com/aroff/cli-framework/issues/29)).

### Tool surface

Tools are derived from the cli-framework `Command` registry (`build_app`) — every command in `REGISTERED_COMMAND_IDS` becomes an MCP tool with name = command id verbatim (e.g. `run`, `init`, `serve`, `health`, `workflow`, `resume`, `checkpoint`, `artifact`, `webhook`, `monitor`, `batch`, `config`, `doctor`, `runs`, `version`). Argument schemas come from each `CommandSpec.args`. Adding a new Newton command auto-publishes a new MCP tool — there is no per-command MCP wiring.

### Usage

```bash
# Default (loopback, port 8730, /mcp)
newton --mcp-serve --mcp-port 8730

# Custom interface, port, and path
newton --mcp-serve --mcp-host 0.0.0.0 --mcp-port 9100 --mcp-path /tools
```

### Cursor / Claude Desktop integration

```json
{
  "mcpServers": {
    "newton": {
      "command": "newton",
      "args": ["--mcp-serve", "--mcp-port", "8730"]
    }
  }
}
```

### Port-conflict policy

Bind failure exits non-zero with a single line containing `NEWTON-MCP-001` and the failed `host:port`. There is no auto-rebind — pass an alternate `--mcp-port`. Unrecoverable upstream MCP runtime errors after a successful bind surface as `NEWTON-MCP-002`.

### Startup log

A successful bind emits one structured `tracing::info!` event with fields `event="mcp_serve_started"`, `mcp_enabled=true`, `bind_address`, `mcp_path`, and integer `tool_count`. No such event is emitted in non-MCP mode.

## Built-in operators

- [references/gh-operator.md](references/gh-operator.md) — `GhOperator`: GitHub CLI wrapper for PR and project board operations

## References

- [references/configuration.md](references/configuration.md) — `.newton/configs` keys read by Newton (`batch`, `monitor`, `init` stub)
- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/batch.md](references/batch.md)
- [references/monitor.md](references/monitor.md)

**Canonical skill:** agent instructions for Newton CLI are maintained in [gonewton/skill](https://github.com/gonewton/skill) (`newton/`). Prefer `newton <cmd> --help` when behavior differs by version.

Organization-specific shell or YAML that sources the same `.conf` files (extra keys, `develop` wrappers) is **not** documented here; keep that in your own workspace skill or internal docs.

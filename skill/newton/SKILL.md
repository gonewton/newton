---
name: newton
description: Newton CLI for workflow YAML graphs (operators, checkpoints, goal gates), batch plan queues, ailoop human-in-the-loop via HumanApprovalOperator/HumanDecisionOperator, and HTTP APIs via serve. Use when running or resuming workflows, validating or linting workflow files, managing checkpoints or artifacts, configuring .newton/configs, or using `workflow validate`, `workflow lint`, `workflow preview`, `workflow graph`, `workflow resume`, `workflow runs`, `workflow checkpoint`, `workflow artifact`, webhook, or batch.
license: Apache-2.0
compatibility: Requires the newton binary on PATH. newton init requires aikit on PATH for templates.
---

# Newton

Newton is a **workflow-first** CLI: YAML workflow graphs with operators, checkpoints, artifacts, and goal gates. Classic evaluator, advisor, and executor-only loops are expressed inside workflows now, not as separate top-level commands. **Sub-workflows** are supported: a task can invoke another workflow file with `WorkflowOperator` (`workflow_path`, optional `context` and `triggers` merges), subject to workspace path rules and a maximum nesting depth.

## When to use

- Running or resuming workflows (including graphs that call nested workflows via `WorkflowOperator`), batch plan queues, or webhook-driven runs.
- Initializing a workspace (`newton init`) and editing `.newton/configs/*.conf`.
- Validating or explaining workflow YAML; cleaning checkpoints or artifacts.
- Operating `newton serve` for HTTP or WebSocket APIs.

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
| `workflow validate` | Validate workflow YAML before run |
| `workflow graph` | Emit Graphviz DOT for the workflow graph (`--format dot --output <PATH>`) |
| `workflow lint` | Best-practice checks on a workflow file |
| `workflow preview` | Human-readable description of workflow behavior |
| `workflow resume` | Continue from a checkpoint (`--run-id`) |
| `workflow runs` | `list` past runs / `show --run-id <RUN_ID>` task replay |
| `workflow checkpoint` | `list` / `clean` checkpoint data |
| `workflow artifact` | `clean` old execution artifacts |
| `webhook` | `serve` or `status` for webhook-triggered runs (`--workflow <PATH>`) |

For commands without a dedicated reference file below, use `newton <cmd> --help` as the source of truth for flags and examples.

There is **no** `step`, `status`, `report`, or `error` subcommand in current releases. Inspect runs via **checkpoints**, **resume**, **artifacts**, workflow logs, and `.newton/tasks/` under the project workspace. See [references/step.md](references/step.md) and related stubs for migration hints.

## Typical flows

1. **New workspace**: `newton init .` then set `workflow_file` in `.newton/configs/default.conf` when using batch; run workflows with `newton run path/to/workflow.yaml --workspace .`.
2. **Queue of plans**: Configure `.newton/configs/<project_id>.conf` with `project_root` and `workflow_file`; place plans in `.newton/plan/<project_id>/todo/`; run `newton batch <project_id>`.
3. **Live HIL**: Use `HumanApprovalOperator` or `HumanDecisionOperator` in your workflow YAML to pause for human input via [ailoop](https://github.com/goailoop/ailoop). Interact with ailoop channels using ailoop's own clients.
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
newton workflow resume --run-id <uuid> --workspace .
```

## MCP Server Mode

Newton exposes every registered command as an MCP (Model Context Protocol) tool. Two deployment topologies are supported.

### Option A — Single-port (`newton serve --with-mcp`) _(recommended)_

Mount the MCP HTTP router on the **same listener** as the Newton REST API. One process, one port, one client URL.

```bash
newton serve --host 127.0.0.1 --port 8080 --with-mcp --mcp-path /mcp
# REST:  http://127.0.0.1:8080/health
# MCP:   http://127.0.0.1:8080/mcp
```

| Flag | Default | Description |
| --- | --- | --- |
| `--with-mcp` | off | Opt-in; absent leaves `serve` behavior unchanged |
| `--mcp-path` | `/mcp` | Path prefix for the MCP endpoint (must start with `/`, must not collide with a REST route) |

**Cursor / Claude Desktop integration (single-port HTTP):**

```json
{
  "mcpServers": {
    "newton": {
      "url": "http://127.0.0.1:8080/mcp",
      "transport": "http"
    }
  }
}
```

**Failure modes:** `NEWTON-SERVE-MCP-001` — invalid `--mcp-path`; `NEWTON-SERVE-MCP-002` — path collides with an existing REST route; `NEWTON-SERVE-MCP-004` — MCP router construction failed.

### Option B — Dedicated MCP-only process (`newton --mcp-serve`)

`--mcp-serve` is **a top-level mode**, not a subcommand argument. It short-circuits subcommand dispatch and binds a separate MCP-only listener. Use this when you do not want the REST API running.

| Flag | Default | Description |
| --- | --- | --- |
| `--mcp-serve` | off | Enable MCP server mode |
| `--mcp-host` | `127.0.0.1` | Bind address for the Streamable HTTP listener |
| `--mcp-port` | `8730` | Distinct from `newton serve` (8080) to avoid collision |
| `--mcp-path` | `/mcp` | HTTP path prefix for the MCP endpoint |

```bash
# Default (loopback, port 8730, /mcp)
newton --mcp-serve --mcp-port 8730

# Custom interface, port, and path
newton --mcp-serve --mcp-host 0.0.0.0 --mcp-port 9100 --mcp-path /tools
```

**Cursor / Claude Desktop integration (dedicated process):**

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

### Tool surface

Newton uses `McpToolExportPolicy::ExposeMcpOnly`; only commands in `MCP_EXPOSED_COMMAND_IDS` become MCP tools. The four exposed tools are: `config`, `health`, `run`, `workflow`. `resume` and `runs` were removed from the MCP tool list in issue #305 — they are now subcommands of `workflow`. `checkpoint` and `artifact` were never MCP-exposed. Adding a new Newton command does **not** automatically expose it as an MCP tool — it must have `expose_mcp: true` and appear in `MCP_EXPOSED_COMMAND_IDS`.

### Port-conflict policy

Bind failure (Option B) exits non-zero with a single line containing `NEWTON-MCP-001` and the failed `host:port`. There is no auto-rebind — pass an alternate `--mcp-port`. Unrecoverable upstream MCP runtime errors after a successful bind surface as `NEWTON-MCP-002`.

### Startup log

A successful bind emits one structured `tracing::info!` event with fields `event="mcp_serve_started"`, `mcp_enabled=true`, `bind_address`, `mcp_path`, and integer `tool_count`. No such event is emitted in non-MCP mode.

## Built-in operators

- [references/gh-operator.md](references/gh-operator.md) — `GhOperator`: GitHub CLI wrapper for PR and project board operations

## References

- [references/configuration.md](references/configuration.md) — `.newton/configs` keys read by Newton (`batch`, `init` stub)
- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/batch.md](references/batch.md)

**Canonical skill:** agent instructions for Newton CLI are maintained in [gonewton/skill](https://github.com/gonewton/skill) (`newton/`). Prefer `newton <cmd> --help` when behavior differs by version.

Organization-specific shell or YAML that sources the same `.conf` files (extra keys, `develop` wrappers) is **not** documented here; keep that in your own workspace skill or internal docs.

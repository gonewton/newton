---
name: newton
description: Newton CLI for workflow YAML graphs and versioned generic Optimization Definitions. `optimize` runs durable evaluate → propose → optional execute → evaluate → decide cycles over identifiable candidate states, with JSON history, constraints, stopping, and requirements revisions. Use when running workflows or authoring, validating, running, resuming, or observing optimization definitions.
license: Apache-2.0
compatibility: Requires the newton binary on PATH.
---

# Newton

Newton is a **workflow-first** CLI **and an autonomous optimizer**: it runs YAML workflow graphs (operators, checkpoints, artifacts, goal gates) and drives definition-bound optimization toward a numeric or Grade objective. **Sub-workflows** are supported: a task can invoke another workflow file with `WorkflowOperator` (`workflow_path`, optional `context` and `triggers` merges), subject to workspace path rules and a maximum nesting depth.

> **Vocabulary changes (pre-1.0):** `batch` was renamed to **`optimize`** (ADR 0003); the `webhook` command and `health` CLI command were **removed** (`webhook` per ADR 0004 — the optimizer is self-driving, no external ingress; `health` folded into `doctor`). The durable work entity `Opportunity` was renamed to **`Finding`** (061). See [Optimization loop](#optimization-loop) and `CONTEXT.md`.

## When to use

- Running or resuming workflows (including graphs that call nested workflows via `WorkflowOperator`).
- Driving a definition-bound **optimization loop** for a local candidate context (`newton optimize <project> --definition <file>`) or reading its JSON history.
- Initializing a workspace (`newton init`) and editing `.newton/configs/*.conf`.
- Validating or explaining workflow YAML; cleaning checkpoints or artifacts.
- Operating `newton serve` for HTTP or WebSocket APIs (incl. the optimize-run + grading read endpoints).
- Working with optional catalog entities such as **Finding**, **Change Request**, and **Plan** via `newton data` or `/api/v1`; these are not prerequisites for generic optimization.
- Authoring an evaluator that returns measurements, checks, evidence, and optional assessment-local observations.

## Installation

```bash
brew tap gonewton/cli
brew install newton

scoop bucket add gonewton https://github.com/gonewton/scoop-bucket
scoop install newton
```

Verify: `newton --help` and `newton --version`.

> **Deprecated:** Manually editing agent config files (`.cursor/mcp.json`, `~/.claude.json`, etc.) to register Newton as an MCP server is deprecated. Use `newton mcp install` instead (see [MCP agent registration](#mcp-agent-registration) below).

## Quick start

1. `newton --help` and `newton <command> --help` for flags.
2. `newton init [PATH]` to create `.newton/` and install the template via the bundled aikit-sdk (PATH defaults to the current directory; `--template builtin` works offline).
3. `newton workflow run <workflow.yaml> --workspace <root>` (optional second positional input file for trigger payload).

## CLI commands (source order)

These subcommands match the current CLI (confirm with `newton --help` on your build):

| Command | Role |
| --- | --- |
| `init` | Create `.newton/` and install the default template |
| `optimize` | Run a durable generic loop. Every definition needs an evaluator and `propose`; `execute` is optional. Qualified candidates and immutable Cycle evidence are retained as JSON. |
| `serve` | HTTP/WebSocket API for workflow state, streaming, and loop observation (see [serve API](references/serve-api.md)) |
| `data` | Catalog CRUD over HTTP-style verbs (`get`/`post`/`patch`/`put`/`delete`) for entities incl. `finding`, `change-request`, `plan`, `optimize-run`, `optimize-cycle`, `eval-run`, `grade` |
| `dependency discover\|inspect\|approve\|impact` | Discover Cargo facts, package human-reviewed Baselines, and query deterministic target-scoped Impact Sequences; see [dependency planning](references/dependency.md). |
| `doctor` | Environment readiness diagnostics (replaces the removed `health` command) |
| `workflow run` | Execute a workflow graph from YAML (see [run](references/run.md)) |
| `workflow validate` | Validate workflow YAML before run |
| `workflow graph` | Emit Graphviz DOT for the workflow graph (`--format dot --output <PATH>`) |
| `workflow lint` | Best-practice checks on a workflow file |
| `workflow preview` | Human-readable description of workflow behavior |
| `workflow resume` | Continue from a checkpoint (`--run-id`) |
| `workflow runs` | `list` past runs / `show --run-id <RUN_ID>` task replay |
| `workflow checkpoint` | `list` / `clean` checkpoint data |
| `workflow artifact` | `clean` old execution artifacts |

> **Removed:** `webhook` (ADR 0004 — no external HTTP ingress; the optimizer is self-driving) and `health` (folded into `doctor`). Don't reference them.

For commands without a dedicated reference file below, use `newton <cmd> --help` as the source of truth for flags and examples.

There is **no** `step`, `status`, `report`, or `error` subcommand in current releases. Inspect runs via **checkpoints**, **resume**, **artifacts**, workflow logs, and `.newton/tasks/` under the project workspace. See [references/step.md](references/step.md) and related stubs for migration hints.

## Typical flows

1. **New workspace**: `newton init .`; run workflows with `newton workflow run path/to/workflow.yaml --workspace .`.
2. **Optimization loop**: write or select a versioned definition, set `definition_file` in `.newton/configs/<project_id>.conf`, then run `newton optimize <project_id> --once`. Use `--resume <RUN_ID>` to continue a safe durable phase; `--requirements-update <file>` is local-only and only activates at a safe boundary.
3. **Live HIL**: Use `HumanApprovalOperator` or `HumanDecisionOperator` in your workflow YAML to pause for human input via [ailoop](https://github.com/goailoop/ailoop). Interact with ailoop channels using ailoop's own clients.
4. **API / dashboards**: `newton serve` exposes REST, WebSocket, and SSE endpoints for workflow instances, streams, and the optimization loop (`/api/v1/optimize-runs`, trajectory, findings), and serves the **embedded web UI** at `/` by default (open the URL printed on startup in a browser to visualize optimize runs, findings, change requests, and plans; `--no-web` disables it). See [references/serve-api.md](references/serve-api.md) and `openapi/newton-api.yaml`.
5. **Grade a project (Finding ingest)**: Write a **command-Grader** at `.newton/grader/<name>/generate.sh <repo_id> <repo_path>` that runs your analyzer (e.g. `dk review`) and **prints an Assessment JSON to stdout** (it must NOT self-persist). The loop's grade phase runs it via `GraderCommandOperator`, which validates and persists the Assessment; `ReconcileOperator` then turns its Observations into durable **Findings**.

## Usage notes

- `newton init` does not need `aikit` on `PATH` (templates install via the bundled aikit-sdk) and refuses to run if `.newton` already exists (remove it or pick another directory). It needs network access to GitHub unless `--template` is `builtin` or a local path.
- `newton workflow run` takes the workflow path as the required first positional argument; the top-level `newton run` and its `--file` flag are gone.
- `--server <URL>` on `newton workflow run` registers the run with a Newton API instance started via `newton serve` for lifecycle notifications.
- Checkpoint and artifact layouts live under `.newton/` inside the workspace you pass with `--workspace` (or the discovered project root).

## Optimization loop

Newton optimizes an Objective over identifiable candidate states:

```text
evaluate incumbent → propose → [execute] → evaluate candidate → decide
```

- Start from an **existing schema-version-2 Optimization Definition**. Create or
  edit definitions with a coding agent and this skill; Newton does not generate
  them from a free-form goal.
- Use `measurement_driven` when a strategy can propose candidates from numeric or
  Grade measurements. It needs no observations, Plan, execute workflow, Git, or
  database.
- Use `observation_driven` when evaluator suggestions guide the next attempt.
  Select at most `max_suggestions` observations from the current assessment;
  the default is five. Findings and cross-assessment links are optional.
- The evaluator returns the exact candidate, correlated evaluation, measurements,
  constraints, and optional assessment. A Grade is one 0–100 measurement type;
  native numeric values retain their units and minimize/maximize direction.
- `propose` returns a ready candidate, an attempt requiring optional `execute`,
  no action, or a known-safe failure. A Plan is optional data on a proposal.
- A candidate replaces the incumbent only after re-evaluation proves improvement
  and every acceptance constraint passes. Unknown evidence never passes.
- Completion, stagnation, no action, limits, regression, cancellation, and
  operational failure are distinct stop results. Retained historical results are
  separate from qualification under the active requirements revision.
- History is authoritative JSON at `.newton/state/optimize/<RUN_ID>/`: immutable
  `run.json` and `cycles/*.json`, mutable `current.json`, then `outcome.json` and
  `report.json`. The standard optimizer does not require SQLite.
- Delivery is domain-specific. A coding workflow may create a review branch; a
  simulator may retain a parameter candidate. Do not add `workflows.promote`.

Read [references/optimize.md](references/optimize.md) before authoring a
definition, and [references/configuration.md](references/configuration.md) for
binding and authority.

## Quick reference

```bash
newton workflow run workflow.yaml --workspace . --verbose
newton optimize my-project --definition objective.yaml --once
newton optimize my-project --resume <RUN_ID> --requirements-update update.yaml
newton workflow validate workflow.yaml
newton workflow lint workflow.yaml
newton workflow preview workflow.yaml
newton workflow resume --run-id <uuid> --workspace .
curl -s localhost:8080/api/v1/optimize-runs            # observe loop runs
```

## MCP agent registration

Newton can register itself as an MCP server in any supported agent with a single command. This replaces manual config-file editing, which is deprecated.

**Discover supported agents and their config file paths:**

```bash
newton mcp list
```

**Register for Cursor (project scope — writes `.cursor/mcp.json` in CWD):**

```bash
newton mcp install --agent cursor --stdio --scope project --overwrite
```

**Register for Claude Code (project scope — writes `.mcp.json` in CWD):**

```bash
newton mcp install --agent claude --stdio --scope project --overwrite
```

**Preview the config entry without writing any file:**

```bash
newton mcp install --agent cursor --stdio --dry-run
```

**Register for other agents (global scope):**

```bash
newton mcp install --agent gemini --stdio --scope global --overwrite
newton mcp install --agent copilot --stdio --scope global --overwrite
newton mcp install --agent opencode --stdio --scope global --overwrite
newton mcp install --agent codex --stdio --scope global --overwrite
```

`newton mcp register` is an alias for `newton mcp install`.

After running `mcp install`, reload the agent (restart or re-open the workspace). Newton's explicitly exposed MCP tools will be callable over the registered stdio transport; see the tool surface below.

| Agent flag | Project-scope config file | Global-scope config file |
| --- | --- | --- |
| `claude` | `.mcp.json` in CWD | `~/.claude.json` |
| `cursor` | `.cursor/mcp.json` in CWD | `~/.cursor/mcp.json` |
| `gemini` | `.gemini/settings.json` in CWD | `~/.gemini/settings.json` |
| `copilot` / `vscode` | `.vscode/mcp.json` in CWD | `~/.config/Code/User/mcp.json` |
| `opencode` | `opencode.json` in CWD | `~/.config/opencode/opencode.json` |
| `codex` | `.codex/config.toml` in CWD | `~/.codex/config.toml` |

## MCP Server Mode

Newton exposes explicitly selected commands as MCP (Model Context Protocol) tools. Two deployment topologies are supported.

### Option A — Single-port (`newton serve --with-mcp`) _(recommended)_

Mount the MCP HTTP router on the **same listener** as the Newton REST API. One process, one port, one client URL.

```bash
newton serve --host 127.0.0.1 --port 8080 --with-mcp
# Web UI: http://127.0.0.1:8080/
# REST:   http://127.0.0.1:8080/healthz
# MCP:    http://127.0.0.1:8080/mcp
```

The MCP endpoint is always mounted at `/mcp`. `--with-mcp` is opt-in; without it `serve` behavior is unchanged. It sits behind the same OIDC layer as the REST API, so this is the only supported way to expose MCP beyond loopback (`--oidc-issuer` / `--oidc-audience`).

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

**Failure mode:** `NEWTON-SERVE-MCP-004` — MCP router construction failed.

### Option B — Dedicated MCP-only process (`newton mcp serve`)

Runs MCP without the REST API. It has **no authentication**, so it binds loopback only: a non-loopback `--host` is refused with `NEWTON-MCP-003` (use Option A with OIDC for remote access).

| Flag | Default | Description |
| --- | --- | --- |
| `--transport` | `http` | `http` (Streamable HTTP) or `stdio` |
| `--host` | `127.0.0.1` | Loopback address only (`127.0.0.1`, `::1`, `localhost`) |
| `--port` | `8730` | Distinct from `newton serve` (8080) to avoid collision |
| `--path` | `/mcp` | HTTP path prefix for the MCP endpoint |

```bash
# Streamable HTTP on loopback, port 8730, /mcp
newton mcp serve

# stdio (what `newton mcp install --stdio` registers)
newton mcp serve --transport stdio
```

**Agent config (stdio, dedicated process):**

```json
{
  "mcpServers": {
    "newton": {
      "command": "newton",
      "args": ["mcp", "serve", "--transport", "stdio"]
    }
  }
}
```

### Tool surface

Newton uses `McpToolExportPolicy::ExposeMcpOnly`; the exposed commands are `config`,
`workflow`, `data.get`, `data.post`, `data.put`, `data.patch`, `data.delete`,
`dependency.inspect`, and `dependency.impact`. Tool names use underscores, for
example `newton_dependency_impact`. `dependency.approve` is not an agent tool:
agents MUST NOT invent a human review record or approve their own dependency map.
Adding a command does not automatically expose it; its `expose_mcp` flag and
`MCP_EXPOSED_COMMAND_IDS` must agree.

### Port-conflict policy

Bind failure (Option B) exits non-zero with a single line containing `NEWTON-MCP-001` and the failed `host:port`. There is no auto-rebind — pass an alternate `--mcp-port`. Unrecoverable upstream MCP runtime errors after a successful bind surface as `NEWTON-MCP-002`.

### Startup log

A successful bind emits one structured `tracing::info!` event with fields `event="mcp_serve_started"`, `mcp_enabled=true`, `bind_address`, `mcp_path`, and integer `tool_count`. No such event is emitted in non-MCP mode.

## Built-in operators

- [references/gh-operator.md](references/gh-operator.md) — `GhOperator`: GitHub CLI wrapper for PR and project board operations

## References

- [references/configuration.md](references/configuration.md) — `.newton/configs` keys read by Newton
- [references/init.md](references/init.md)
- [references/run.md](references/run.md) — `newton workflow run` arguments and options
- [references/serve-api.md](references/serve-api.md) — `newton serve` REST/stream map, auth, storage, OpenAPI pointer
- [references/optimize.md](references/optimize.md) — the `optimize` command + the closed optimization loop, entities, break conditions, and `serve` endpoints (supersedes the old `batch.md`)
- [references/dependency.md](references/dependency.md) — approved local Baselines and deterministic planner queries

**Canonical skill:** this in-tree copy (`skill/newton/` in [gonewton/newton](https://github.com/gonewton/newton)) is the only maintained source; the standalone `gonewton/skill` repository is retired. Install with `fastskill add https://github.com/gonewton/newton/tree/main/skill/newton`. Prefer `newton <cmd> --help` when behavior differs by version.

Organization-specific shell or YAML that sources the same `.conf` files (extra keys, `develop` wrappers) is **not** documented here; keep that in your own workspace skill or internal docs.

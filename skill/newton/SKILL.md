---
name: newton
description: Newton CLI for workflow YAML graphs (operators, checkpoints, goal gates), the autonomous optimization loop (grade → reconcile → change-request → plan → develop → re-grade) over the Finding → Change Request → Plan → Execution spine with Graders/Assessments, ailoop human-in-the-loop via HumanApprovalOperator/HumanDecisionOperator, and HTTP APIs via serve. Use when running or resuming workflows, driving or observing the optimization loop, validating or linting workflow files, managing checkpoints or artifacts, configuring .newton/configs, or using `optimize`, `workflow validate`, `workflow lint`, `workflow preview`, `workflow graph`, `workflow resume`, `workflow runs`, `workflow checkpoint`, `workflow artifact`.
license: Apache-2.0
compatibility: Requires the newton binary on PATH. newton init requires aikit on PATH for templates.
---

# Newton

Newton is a **workflow-first** CLI **and an autonomous optimizer**: it runs YAML workflow graphs (operators, checkpoints, artifacts, goal gates) and drives the **optimization loop** that improves a project toward a **Grade**. **Sub-workflows** are supported: a task can invoke another workflow file with `WorkflowOperator` (`workflow_path`, optional `context` and `triggers` merges), subject to workspace path rules and a maximum nesting depth.

> **Vocabulary changes (pre-1.0):** `batch` was renamed to **`optimize`** (ADR 0003); the `webhook` command and `health` CLI command were **removed** (`webhook` per ADR 0004 — the optimizer is self-driving, no external ingress; `health` folded into `doctor`). The durable work entity `Opportunity` was renamed to **`Finding`** (061). See [Optimization loop](#optimization-loop) and `CONTEXT.md`.

## When to use

- Running or resuming workflows (including graphs that call nested workflows via `WorkflowOperator`).
- Driving the **optimization loop** for a project (`newton optimize <project>` / the `.newton/scripts/optimize.sh` loop driver) or **observing** it over `serve`.
- Initializing a workspace (`newton init`) and editing `.newton/configs/*.conf`.
- Validating or explaining workflow YAML; cleaning checkpoints or artifacts.
- Operating `newton serve` for HTTP or WebSocket APIs (incl. the optimize-run + grading read endpoints).
- Working with loop entities — **Finding**, **Change Request**, **Plan**, **Assessment** — via `newton data` or `/api/v1` (e.g. `newton data post finding`, `POST /api/v1/findings/{id}/unblock`).
- Wiring a **Grader** (`.newton/grader/<name>/generate.sh`) that prints an **Assessment** to stdout for the loop's grade phase.

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
2. `newton init [PATH]` to create `.newton/` and install the template via `aikit` (PATH defaults to the current directory).
3. `newton run <workflow.yaml> --workspace <root>` (optional second positional input file for trigger payload).

## CLI commands (source order)

These subcommands match the current CLI (confirm with `newton --help` on your build):

| Command | Role |
| --- | --- |
| `run` | Execute a workflow graph from YAML |
| `init` | Create `.newton/` and install the default template |
| `optimize` | Drive the optimization loop for a project (renamed from `batch`, ADR 0003). The current Rust command drains the **Plan** queue under `.newton/plan/<project_id>/`; the **full closed loop** (grade → reconcile → change-request → plan → develop → re-grade) is driven by `.newton/scripts/optimize.sh` until the in-process driver lands (spec 073) |
| `serve` | HTTP/WebSocket API for workflow state, streaming, and loop observation |
| `data` | Catalog CRUD over HTTP-style verbs (`get`/`post`/`patch`/`put`/`delete`) for entities incl. `finding`, `change-request`, `plan`, `optimize-run`, `optimize-cycle`, `eval-run`, `grade` |
| `doctor` | Environment readiness diagnostics (replaces the removed `health` command) |
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

1. **New workspace**: `newton init .` then set `workflow_file` in `.newton/configs/default.conf`; run workflows with `newton workflow run path/to/workflow.yaml --workspace .`.
2. **Optimization loop**: Configure `.newton/configs/<project_id>.conf` with the `optimize_*` block (repo, test cmd, graders, break-condition thresholds — see [Optimization loop](#optimization-loop)); run the closed loop with `.newton/scripts/optimize.sh <project_id> [--once]`. (The Rust `newton optimize <project_id>` currently drains the Plan queue.)
3. **Live HIL**: Use `HumanApprovalOperator` or `HumanDecisionOperator` in your workflow YAML to pause for human input via [ailoop](https://github.com/goailoop/ailoop). Interact with ailoop channels using ailoop's own clients.
4. **API / dashboards**: `newton serve` exposes REST, WebSocket, and SSE endpoints for workflow instances, streams, and the optimization loop (`/api/v1/optimize-runs`, trajectory, findings) — see `newton serve --help` and the Newton `README.md`.
5. **Grade a project (Finding ingest)**: Write a **command-Grader** at `.newton/grader/<name>/generate.sh <repo_id> <repo_path>` that runs your analyzer (e.g. `dk review`) and **prints an Assessment JSON to stdout** (it must NOT self-persist). The loop's grade phase runs it via `GraderCommandOperator`, which validates and persists the Assessment; `ReconcileOperator` then turns its Observations into durable **Findings**.

## Usage notes

- `newton init` requires `aikit` on `PATH` and refuses to run if `.newton` already exists (remove it or pick another directory).
- `newton run` takes the workflow path as the required first positional argument; the legacy named flag is gone.
- `--server <URL>` on `newton run` registers the run with a Newton API instance started via `newton serve` for lifecycle notifications.
- Checkpoint and artifact layouts live under `.newton/` inside the workspace you pass with `--workspace` (or the discovered project root).

## Optimization loop

Newton's reason to exist is an **autonomous, GitHub-free loop** that improves a project toward a project-defined **Grade**. One pass:

```
grade ─→ reconcile ─→ change-request ─→ (approve) ─→ plan ─→ develop ─→ merge ─┐
(Assessment)(Findings)(Change Request)   (HIL/auto)  (HOW)  (tests+commit) local│
                            │ decision=none + nothing blocked → CONVERGED       │ re-grade
                            └───────────────────────────────────────────────────┘
```

**Durable spine (lives in Newton's store, never a board):** `Finding → Change Request → Plan → Execution`.

**Key concepts** (full glossary in `CONTEXT.md`):

- **Grader / Assessment / Score / Observation** — a **Grader** (a command program, e.g. `generate.sh`, or a rubric agent) inspects the repo and emits an **Assessment**: an overall **Grade** (0–100), per-dimension **Scores**, and **Observations** (the text-gradient: problem + why + recommended action).
- **Reconciliation → Finding** — `ReconcileOperator` matches an Assessment's Observations against open **Findings** (the durable, triageable records); match refreshes, no-match creates, open-with-no-match **resolves**. Identity is Newton's, never the grader's.
- **Change Request** — the synthesized proposal (`ChangeRequestOperator`) over the standing Findings (the WHAT/WHY). `decision: propose | none`.
- **Plan** — the enriched implementation spec (the HOW). Status: `draft → ready → running → complete | failed`, plus `abandoned`. Fields: `body`, `executionId`, `attempts`, `lastError`, `module`.
- **Optimize Run / Cycle / Trajectory** — one loop invocation is an **Optimize Run**; each iteration is a **Cycle**; the per-cycle audit log (grades, decision, plan, develop status) is the **Trajectory**. A Run contains Cycles; each Cycle fires several **Steps** (workflow runs) and one develop **Execution**.
- **Break conditions (the loop MUST terminate)** — `converged` (decision none for K rounds, zero blocked) · `stalled_on_blocked` · `max_cycles` · per-grader `target` (all clear) · per-grader `regression` (any drops) · `no_progress`.
- **`blocked` Finding + un-block** — when a Plan fails develop after `optimize_max_failed_attempts`, its Findings are **quarantined** (`blocked`) and a human is escalated to; the loop keeps optimizing the rest. Clear with `POST /api/v1/findings/{id}/unblock`.
- **Multi-grader** — `optimize_graders` is a space list; Findings pool into one Change Request per cycle; targets/regression are per-grader.

**Drivers:** the **closed loop** runs via `.newton/scripts/optimize.sh <project_id> [--once] [--max-cycles N] [--delivery local|pr] [--auto-approve]` (interim; the in-process `newton optimize` is spec 073). It reads the `optimize_*` block in `.newton/configs/<project_id>.conf` (`optimize_repo_id`, `optimize_repo_path`, `optimize_test_cmd`, `optimize_graders`, `optimize_max_cycles`, `optimize_converge_rounds`, `optimize_target_grade[_<grader>]`, `optimize_regression_tolerance[_<grader>]`, `optimize_max_failed_attempts`, `optimize_auto_approve`, `delivery`).

**Observe over `serve`** (read-only — the loop is self-driving, ADR 0004):

```bash
GET  /api/v1/optimize-runs                      # list runs (status, cycle, per-grader grades, open/blocked)
GET  /api/v1/optimize-runs/{id}                 # one run + outcome reason
GET  /api/v1/optimize-runs/{id}/trajectory      # per-cycle rows
GET  /api/v1/findings?status=blocked            # blocked findings (inline plan/attempts/lastError/CR)
POST /api/v1/findings/{id}/unblock              # return a blocked Finding to the actionable pool (409 if not blocked)
```

See [references/optimize.md](references/optimize.md).

## Quick reference

```bash
newton workflow run workflow.yaml --workspace . --verbose
.newton/scripts/optimize.sh my-project --once          # drive one closed-loop cycle
newton optimize my-project --workspace ~/ws --once     # Rust command: drain the Plan queue
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

After running `mcp install`, reload the agent (restart or re-open the workspace). Newton's exposed MCP tools (`config`, `health`, `run`, `workflow`) will be callable over the registered stdio transport.

| Agent flag | Project-scope config file | Global-scope config file |
| --- | --- | --- |
| `claude` | `.mcp.json` in CWD | `~/.claude.json` |
| `cursor` | `.cursor/mcp.json` in CWD | `~/.cursor/mcp.json` |
| `gemini` | `.gemini/settings.json` in CWD | `~/.gemini/settings.json` |
| `copilot` / `vscode` | `.vscode/mcp.json` in CWD | `~/.config/Code/User/mcp.json` |
| `opencode` | `opencode.json` in CWD | `~/.config/opencode/opencode.json` |
| `codex` | `.codex/config.toml` in CWD | `~/.codex/config.toml` |

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

- [references/configuration.md](references/configuration.md) — `.newton/configs` keys read by Newton (`optimize_*` block, `init` stub)
- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/optimize.md](references/optimize.md) — the `optimize` command + the closed optimization loop, entities, break conditions, and `serve` endpoints (supersedes the old `batch.md`)

**Canonical skill:** agent instructions for Newton CLI are maintained in [gonewton/skill](https://github.com/gonewton/skill) (`newton/`). Prefer `newton <cmd> --help` when behavior differs by version.

Organization-specific shell or YAML that sources the same `.conf` files (extra keys, `develop` wrappers) is **not** documented here; keep that in your own workspace skill or internal docs.

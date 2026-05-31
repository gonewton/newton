# Newton

**Newton** is a workflow-first CLI for deterministic automation and orchestration. You define steps in YAML (shell commands, agents, human approvals, branching, nested workflows), and Newton runs them with explicit completion rules, checkpoints, and artifacts. It fits agent-assisted coding, release checklists, and batch plan queues where you want a defined graph instead of ad hoc scripts.

Version: **0.5.109** · Repository: [github.com/gonewton/newton](https://github.com/gonewton/newton)

## Installation

### macOS / Linux (Homebrew)

```bash
brew tap gonewton/cli
brew install newton
```

### Windows (Scoop)

```powershell
scoop bucket add gonewton https://github.com/gonewton/scoop-bucket
scoop install newton
```

Verify: `newton --version` and `newton --help`.

## Prerequisites

- The **Newton CLI** (installed above).
- **Optional**: Git for version control, hooks, and batch workflows.

`newton init .` scaffolds a workspace and installs the default template via the bundled **aikit-sdk** (statically linked). You do **not** need the `aikit` binary on your `PATH` for init.

## Quick start

1. Create a project directory and `cd` into it.
2. Initialize the workspace:

   ```bash
   newton init .
   ```

3. Run a workflow:

   ```bash
   newton workflow run path/to/workflow.yaml --workspace .
   ```

For an existing repository, run `newton init .` at the repo root. Edit `.newton/configs/default.conf` to set `workflow_file` when using batch mode (see [Batch mode](#batch-mode) below).

## What you get

Newton runs YAML workflow graphs with:

- **Operators**: shell commands, context patches, assertions, nested workflows, human approval/decision gates, GitHub CLI actions, and agent engines (availability depends on your workflow; run `newton workflow preview <file>` to see the resolved operator list).
- **Safety**: lint, validate, and preview before run; guarded shell usage and reachability checks.
- **Durability**: checkpoint persistence, resume, artifact routing, and execution history under `.newton/`.
- **Authoring**: macros, `include_if` filtering, `{{ ... }}` interpolation, and `$expr` evaluation.

Built-in operators include `CommandOperator`, `WorkflowOperator` (nested workflows), `HumanApprovalOperator`, `HumanDecisionOperator`, and `GhOperator`. Agent operators integrate with **aikit-sdk**; quota exhaustion surfaces as error code `WFG-AGENT-008` (provider-agnostic detection via aikit-sdk, not by parsing agent output).

For operator reference, see [docs/operators/](docs/operators/) and the [Newton skill](skill/newton/SKILL.md) (`skill/newton/references/`).

## Common commands

| Command | Purpose |
| --- | --- |
| `newton workflow run <file>` | Execute a workflow graph |
| `newton workflow validate\|lint\|preview\|graph` | Check or explain a workflow before run |
| `newton workflow resume --run-id <UUID>` | Continue from a checkpoint |
| `newton workflow runs list\|show` | Inspect past executions |
| `newton workflow checkpoint\|artifact` | Manage checkpoints and artifacts |
| `newton init [path]` | Scaffold `.newton/` and install template |
| `newton batch <project_id>` | Process queued plan files |
| `newton serve` | HTTP/WebSocket API for workflow state and integrations |
| `newton webhook serve\|status` | Trigger workflows from external HTTP events |

Run `newton <command> --help` for flags and examples. The top-level `newton run` command is deprecated; use `newton workflow run`.

### Workflow run (minimal example)

```bash
newton workflow run workflow.yaml
newton workflow run workflow.yaml --workspace ./output --trigger env=prod
newton workflow run workflow.yaml --timeout 3600 --parallel-limit 4 --verbose
```

Trigger payload merge order: `--parameters-json` (base object), then each `--trigger KEY=VAL` in order. Values prefixed with `@` load file contents.

### Batch mode

Batch mode processes markdown plan files from `.newton/plan/<project_id>/todo/` using the workflow named in `.newton/configs/<project_id>.conf`:

```conf
project_root=/path/to/project
workflow_file=path/to/workflow.yaml
```

```bash
newton batch default          # poll and process plans
newton batch default --once     # process one plan then exit
```

Plans move to `completed/` on success or `failed/` on error. See [skill/newton/references/batch.md](skill/newton/references/batch.md) for plan file format and lifecycle.

### HTTP serve API

`newton serve` exposes REST, WebSocket, and SSE endpoints for workflow state, portfolio data, human-in-the-loop, and AI tool sessions:

```bash
newton serve --host 127.0.0.1 --port 8080
newton serve --with-mcp    # mount MCP at /mcp on the same port
```

- **OpenAPI contract**: [openapi/newton-backend-parity.yaml](openapi/newton-backend-parity.yaml)
- **Realtime contract**: [openapi/newton-realtime.asyncapi.yaml](openapi/newton-realtime.asyncapi.yaml)
- **Health**: `GET /healthz` · **API docs**: `GET /api/docs`

REST routes are versioned under `/api/v1/`. Run `newton serve --help` for the full route list.

### MCP mode

Expose Newton commands as MCP tools:

```bash
# Combined REST + MCP (recommended)
newton serve --with-mcp

# Dedicated MCP-only process
newton mcp serve --port 8730
```

See `newton mcp serve --help` for client configuration examples.

### Human-in-the-loop

`HumanApprovalOperator` and `HumanDecisionOperator` pause workflows for human input via [ailoop](https://github.com/goailoop/ailoop). Configure ailoop in `.newton/configs/*.conf` or via `NEWTON_AILOOP_*` environment variables. See [docs/operators/human_approval.md](docs/operators/human_approval.md) and [docs/operators/human_decision.md](docs/operators/human_decision.md).

## Workspace layout

After `newton init`, Newton expects:

```
workspace/
├── .newton/
│   ├── workflows/       # Workflow YAML (from template)
│   ├── configs/         # Batch and integration config (*.conf)
│   ├── plan/            # Batch plan queues by project_id
│   ├── tasks/           # Per-plan execution state
│   ├── state/           # Workflow run records
│   ├── checkpoints/     # Resume checkpoints
│   ├── artifacts/       # Generated artifacts
│   └── logs/            # newton.log
└── (your project files)
```

## Logging

Logs default to `<workspace>/.newton/logs/newton.log` (or `$HOME/.newton/logs/newton.log` when no workspace is detected). Override per invocation with `--log-dir`.

Optional tuning via `.newton/config/logging.toml` and `RUST_LOG` for tracing verbosity. Set `NEWTON_REMOTE_AGENT=1` to keep file logging active while suppressing console output in remote or batch contexts.

Inspect past runs:

```bash
newton workflow runs list
newton workflow runs show --run-id <UUID> --task <TASK_ID>
```

## Further reading

| Resource | Contents |
| --- | --- |
| [skill/newton/SKILL.md](skill/newton/SKILL.md) | Command reference and typical flows |
| [docs/context.md](docs/context.md) | Domain terminology (portfolio, plans, opportunities) |
| [docs/DEPLOY.md](docs/DEPLOY.md) | Deployment notes |
| [CHANGELOG.md](CHANGELOG.md) | Release history |
| [architecture.md](architecture.md) | System design (contributors) |

## License

See [LICENSE](LICENSE) for details.

Contributors: see [CONTRIBUTING.md](CONTRIBUTING.md).

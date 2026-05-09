# Newton

**Newton** is a workflow-first CLI for deterministic automation and orchestration.
- A deterministic workflow runner with linting, explain output, checkpointing, artifacts, goal gates, terminal tasks, and completion policy controls.

## What is Newton?

Newton is a **workflow-first** tool for running structured, repeatable automation: you describe steps in YAML (shell commands, agents, human approvals, branching, and checks), and the CLI runs them with clear completion rules, checkpoints, and artifacts. It fits agent-assisted coding, release checklists, and other tasks where you want a defined graph instead of ad hoc scripts.

You can still think in terms of **evaluate → advise → act** when designing workflows (measure, decide, apply), but the unit of execution is always the workflow file you run with `newton run` or `newton batch`.

## Workflow Graph Capabilities

Newton includes a production workflow runner with YAML-defined tasks and deterministic execution semantics:

- Workflow commands: `newton run`, `newton workflow {validate|lint|preview|graph}`, `newton resume`, `newton runs {list|show}`, `newton checkpoint {list|clean}`, `newton artifact clean`, `newton webhook {serve|status}`
- Safety checks: workflow lint, early validation of expressions, guarded shell usage, reachability checks
- Deterministic completion: goal gates, terminal tasks, explicit completion policy, stable error codes
- Runtime durability: checkpoint persistence, resume support, artifact routing/cleanup, execution warnings
- Authoring support: macros, `include_if` filtering, `{{ ... }}` interpolation, and `$expr` evaluation
- **Sub-workflows**: a task can run another workflow file in-process (`WorkflowOperator`), with optional merged context and trigger payload, workspace-relative path sandboxing, and a configurable nesting depth limit

### Agent quota failure behavior

For SDK-backed `AgentOperator` engines (`claude`, `agent`, `codex`, `gemini`, `opencode`), Newton fails the task with `WFG-AGENT-008` when quota exhaustion is detected. Newton is completely agnostic to provider-specific message formats — all quota detection logic lives in aikit-sdk.

When aikit-sdk detects quota exhaustion, it sets `RunResult.quota_exceeded` and raises `RunError::QuotaExceeded`; Newton maps this directly to workflow error `WFG-AGENT-008`, enriched with artifact paths (events NDJSON and stderr). Consumers of Newton workflows should not attempt to parse provider-specific agent output for quota signals — rely on the `WFG-AGENT-008` error code and its `provider`, `quota_category`, and `raw_excerpt` context fields.

### Built-in Workflow Operators

| Operator | Purpose |
|---|---|
| `NoOpOperator` | Pass-through step; useful for routing and branching |
| `CommandOperator` | Run shell commands; captures stdout/stderr as JSON output |
| `SetContextOperator` | Deep-merge a patch object into the global workflow context |
| `ReadControlFileOperator` | Read and parse a JSON file from a path resolved at runtime |
| `AssertCompletedOperator` | Assert that a set of task IDs have completed before proceeding |
| `WorkflowOperator` | Run a **nested workflow** from another YAML file; merges parent context and triggers into the child, returns child execution summary (for example `child_execution_id`) |
| `HumanApprovalOperator` | Pause for a boolean approve/reject decision from a human operator |
| `HumanDecisionOperator` | Pause for a multiple-choice selection from a human operator |
| `GhOperator` | GitHub CLI wrapper for PR operations (`pr_create`, `pr_view`, `pr_approve`) and project board mutations (`project_resolve_board`, `project_item_set_status`) |

Newton also bundles agent and MCP operator integrations. Their availability depends on the workflow you load — run `newton workflow preview <workflow.yaml>` for the exact operator list resolved by your file. For `GhOperator` reference documentation, see the [Newton skill](/.agents/skills/newton/references/gh-operator.md); for other operators, consult `docs/operators/`.

#### Sub-workflows

Use **`WorkflowOperator`** when you want to reuse a workflow graph or split a large file into smaller ones. In a task, set `operator: WorkflowOperator` and pass:

- **`workflow_path`** (required): path to the child workflow YAML, relative to the **parent** workflow file and restricted to your workspace
- **`context`** (optional): JSON object shallow-merged into the child workflow context (combined with the parent context)
- **`triggers`** (optional): JSON object shallow-merged into the child trigger payload (combined with the parent triggers)

Child runs are tracked separately; the task output includes identifiers such as `child_execution_id` so you can correlate parent and child in logs and checkpoints. Nesting is limited by a maximum depth (default allows typical reuse without unbounded recursion).

## Installation

### macOS / Linux (Homebrew)

First, tap this repository:

```bash
brew tap gonewton/cli
```

Then install the tools:

```bash
brew install newton
```

### Windows (Scoop)

First, add this bucket:

```powershell
scoop bucket add gonewton https://github.com/gonewton/scoop-bucket
```

Then install:

```powershell
scoop install newton
```

## Prerequisites

- **Required**: The Newton CLI itself, installed via the package instructions above. Once the CLI is available, `newton init .` installs the Newton workspace template via the bundled `aikit-sdk` (a statically linked workspace dependency — you do **not** need `aikit` on your `PATH`).
- **Optional**: Git for working with version control, hooks, and batch workflows.
- If `newton init .` cannot complete (missing template, network issues, or template source errors), check your connectivity and that the configured template source is reachable. The default template source is `gonewton/newton-templates`; override with `--template <SOURCE>` to use a different locator or a local path.

## Quick Start

Follow the **Setting up a new project** flow below to go from a blank directory to `newton run` with the default templates and tooling.

### Quick setup: run a simple coding project

1. Create a project directory and `cd` into it.
2. Run `newton init .` to scaffold the workspace.
3. Run a workflow (path comes from your template or your own YAML; optional input file as second positional):
   ```bash
   newton run path/to/workflow.yaml --workspace .
   ```

### Setting up a new project

1. Create a project directory and `cd` into it.
2. Run `newton init .` to scaffold `.newton/` (layout, template files such as workflows and helper scripts, and `.newton/configs/default.conf`).
3. Edit `.newton/configs/default.conf`: set `workflow_file` to the workflow YAML `newton batch` should run (see comment in that file). Add a `<project_id>.conf` copy or symlink if you use batch with a non-default id.
4. Run a workflow explicitly, for example: `newton run path/to/workflow.yaml --workspace .` (use the paths described in your template README).

For an existing repository, run `newton init .` at the repo root instead of creating a new directory.


### Verify your setup

- Before your first run, confirm the CLI is installed by checking `newton --version`.

### CLI Version & Help

```bash
newton --version
newton 0.5.82

$ newton --help
newton 0.5.82
Newton CLI for optimization and workflow automation

Usage: newton <COMMAND>
```

The help output now includes the same version banner at the top, so you can confirm which release is installed even when scanning command descriptions.

## Commands Reference

### `run <workflow.yaml>`

Execute a workflow graph defined in YAML.

**Options:**
- `--workspace <PATH>`: Workspace root directory (default: current directory)
- `--trigger KEY=VALUE`: Merge key into triggers.payload (repeatable; VALUE may be `@path` to load file content as the value)
- `--trigger-file <PATH>`: Load JSON object as base trigger payload before `--trigger` overrides
- `--context KEY=VALUE`: Merge key into workflow context at runtime (repeatable)
- `--timeout SECONDS`: Wall-clock time limit override (in seconds)
- `--parallel-limit N`: Runtime override for bounded task concurrency
- `-v`, `--verbose`: Print task stdout/stderr to terminal after each task completes
- `--server <URL>`: Newton server URL to register this run (optional)

The workflow YAML is supplied as a required positional argument; the legacy named flag has been removed.

**Trigger payload merge order** (left to right): `--trigger-file` is loaded first as the base JSON object, then each `--trigger KEY=VAL` overlays in order. Within a value, `@path` reads the file as a string.

**Examples:**
```bash
# Run with default settings
newton run workflow.yaml

# With workspace and trigger data
newton run workflow.yaml --workspace ./output --trigger key=value

# Multiple trigger overrides
newton run workflow.yaml --trigger env=prod --trigger version=1.2.3

# With time limit
newton run workflow.yaml --timeout 3600
```

### `init [workspace-path]`

Create the `.newton` workspace layout, install the default Newton workspace template, and write `.newton/configs/default.conf` with `project_root` (absolute path to the directory you initialized), a default `coding_model` (typically `zai-coding-plan/glm-4.7`), and commented guidance for `workflow_file` (set this when using `newton batch`).

**Options:**
- `--template <SOURCE>`: Optional template locator (default: `gonewton/newton-templates`). If the value is a path on disk, that template is used; otherwise the built-in template shipped with the CLI is used.

**Examples:**
```bash
newton init .
newton init /path/to/project
```

### `batch <project_id>`

Process queued plan files for a project. `newton batch` discovers the workspace root by walking up from the current directory (or use `--workspace PATH`) until it finds `.newton`. It reads `.newton/configs/<project_id>.conf`, which **must** include `project_root` and `workflow_file` (path to a workflow YAML, resolved relative to `project_root` or the workspace root). Queued items live under `.newton/plan/<project_id>/todo/`. For each plan, Newton runs that workflow the same way as `newton run` on that YAML.

- `--workspace PATH`: Override workspace discovery.
- `--once`: Process one todo file then exit.
- `--poll-interval SECONDS`: Poll interval when the queue is empty (default 60).

Plan files move from `todo` to `completed` when the workflow succeeds, or to `failed` when it errors.

Other keys in `.conf` files are ignored by `newton batch` unless documented for another command.

### `serve`

Serve starts the HTTP/WebSocket API server that provides real-time access to workflow execution state. It powers web UIs, monitoring dashboards, and other backend integrations against a running Newton workspace. CORS is enabled for local development by default.

**Options** (from `crates/cli/src/cli/args.rs`):
- `--host <HOST>`: Host address to bind to (default: `127.0.0.1`).
- `--port <PORT>`: Port to listen on (default: `8080`).
- `--static-ui <PATH>`: Optional path to a built Newton UI dist directory; when present the UI is served alongside the API.

**Example:**
```bash
newton serve --host 0.0.0.0 --port 9000
```

**Endpoint groups:** see `newton serve --help` for an enumeration of route groups, or [`openapi/newton-backend-parity.yaml`](openapi/newton-backend-parity.yaml) for the canonical contract (methods, parameters, and response shapes). Schemas and query parameters are also catalogued in [`skill/newton/references/serve-api.md`](skill/newton/references/serve-api.md).

**Storage:** `newton serve` reads from the parity backend store (default SQLite) defined in [`crates/backend/src/store.rs`](crates/backend/src/store.rs); the schema lives in [`openapi/newton-backend-parity.sqlite.sql`](openapi/newton-backend-parity.sqlite.sql).

**Authoritative contract:** The HTTP/WebSocket/SSE surface is specified in [`openapi/newton-backend-parity.yaml`](openapi/newton-backend-parity.yaml).

### MCP mode

Newton exposes every registered command as an MCP tool. There are two ways to enable MCP, depending on whether you want a dedicated MCP-only process or a combined REST + MCP server on a single port.

#### Option A — Single-port topology (`newton serve --with-mcp`) _(recommended)_

`newton serve --with-mcp` mounts the MCP HTTP router on the **same listener** as the Newton REST API. One process, one port, one URL prefix — simpler firewall rules, simpler TLS termination, simpler client config.

```bash
newton serve --host 127.0.0.1 --port 8080 --with-mcp --mcp-path /mcp
# REST:  curl  http://127.0.0.1:8080/health
# MCP:   POST  http://127.0.0.1:8080/mcp
```

| Flag | Default | Notes |
|---|---|---|
| `--with-mcp` | off | Opt-in; absent leaves `serve` behavior unchanged |
| `--mcp-path` | `/mcp` | HTTP path prefix; must start with `/`, must not be `/` or collide with a REST route |

**Cursor/Claude client config (single-port):**

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

#### Option B — Dedicated MCP-only process (`newton --mcp-serve`)

MCP mode is a top-level mode (not a subcommand) — it short-circuits subcommand dispatch and runs a standalone MCP HTTP listener on a separate port.

| Flag | Default | Notes |
|---|---|---|
| `--mcp-serve` | off | Required to enable MCP-only mode |
| `--mcp-host` | `127.0.0.1` | Loopback only |
| `--mcp-port` | `8730` | Distinct from `newton serve` (8080) |
| `--mcp-path` | `/mcp` | HTTP path prefix |

```bash
# Default (loopback, port 8730, /mcp)
newton --mcp-serve

# Custom interface, port, and path
newton --mcp-serve --mcp-host 0.0.0.0 --mcp-port 9100 --mcp-path /tools
```

**Note on `--help` output.** The current upstream `cli-framework` clap definition prints `--mcp-port [default: 8080]`; Newton transparently rewrites argv to inject `8730` when no explicit port is given so the actual bind matches the table above. **For maximum clarity, always pass `--mcp-port` explicitly** until upstream defaults are aligned.

**Cursor/Claude client config (dedicated process):**

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

**Port-conflict policy.** On bind failure Newton exits non-zero and prints a single line `NEWTON-MCP-001: failed to bind MCP server to <host>:<port>: <os error>`. There is no auto-rebind — pass an alternate `--mcp-port`. An unrecoverable upstream runtime error after a successful bind surfaces as `NEWTON-MCP-002`.

### Ailoop human-in-the-loop integration

The `newton monitor` TUI subcommand has been removed. To interact with ailoop channels, use ailoop's own clients directly (for example `ailoop serve`, `ailoop ask`, and `ailoop say`). The `HumanApprovalOperator` and `HumanDecisionOperator` workflow operators continue to integrate with ailoop for in-workflow human gates.

### `workflow validate <workflow.yaml>`

Validates a workflow YAML for syntax errors, schema compliance, and logical issues before execution (YAML structure, schema, task dependencies, and resource configuration). Returns 0 on success and 1 with errors on stderr.

### `workflow graph <workflow.yaml>`

Renders a Graphviz DOT representation of the workflow graph that can be turned into visual diagrams of task dependencies, parallel execution opportunities, the critical path, and data flow. The current single supported value is `--format dot`. Use `--output graph.dot` (`-o`) and render with `dot -Tpng graph.dot -o workflow.png`.

### `workflow lint <workflow.yaml>`

Analyzes a workflow against Newton's best-practice rules — performance anti-patterns, resource usage, security and maintainability issues, and common workflow design mistakes. Lint warnings are advisory and do not block execution. Use `--format json` for CI integration.

### `workflow preview <workflow.yaml>`

Produces detailed, human-readable documentation about what the workflow does and how it will execute, covering step-by-step flow, dependencies, configuration effects, resource constraints, and expected inputs/outputs. Accepts `--trigger`, `--trigger-file`, `--context`, and `--workspace` to mirror the `run` invocation. Output formats: `text` (default), `prose`, and `json`.

### `resume`

Restarts a workflow execution from its last saved checkpoint, useful after interruptions, parameter changes, or maintenance windows. Requires `--run-id <UUID>`; pass `--allow-workflow-change` to override the default safety check that the workflow definition is unchanged since the checkpoint.

### `checkpoint`

`checkpoint list` and `checkpoint clean` manage saved workflow states that allow resumption after interruption. Use `newton checkpoint list --workspace ./workspace --json` for a machine-readable list and `newton checkpoint clean --workspace ./workspace --older-than 7d` to remove old checkpoints. Checkpoints live under `.newton/checkpoints/` in the workspace.

### `artifact`

`artifact clean` removes the files, logs, and output data generated during workflow execution. Use `newton artifact clean --workspace ./workspace --older-than 7d` to reclaim disk space; retention strings accept days (`7d`), weeks (`1w`), or hours (`24h`). Artifacts live under `.newton/artifacts/`.

### `webhook`

Webhook exposes HTTP endpoints that trigger workflow executions in response to external events (Git hosting services, CI/CD platforms, monitoring/alerting systems, and custom integrations). Use `newton webhook serve --workflow <PATH> --workspace <PATH>` to start the server and `newton webhook status --workflow <PATH> --workspace <PATH>` to inspect configuration.

### `runs`

`runs list` and `runs show` provide access to per-task execution history stored in `.newton/state/workflows/`. Use `newton runs list` to enumerate runs and `newton runs show <RUN_ID>` to display resolved inputs, operators, and outputs for every task. See **Logging → Reviewing execution history** for examples.

### Plans and the batch queue

Newton uses a plan-based queue system for processing multiple tasks through batch mode:

#### Plan File Structure

Plans are stored in `.newton/plan/{project_id}/` and move through different states:

1. **`todo/`** - Plans ready to be processed (picked up automatically by `newton batch`)
2. **`completed/`** - Successfully completed plans
3. **`failed/`** - Plans that failed to complete
4. **`draft/`** - Plans not yet ready for processing

#### Creating Plan Files

Plan files are markdown documents with optional YAML frontmatter:

```markdown
---
branch: feature/add-user-authentication
---

# Add User Authentication

Implement OAuth 2.0 authentication for the user login system.

## Requirements
- Support Google and GitHub OAuth providers
- Store tokens securely
- Add logout functionality
```

The frontmatter `branch` field specifies the git branch name. If omitted, batch mode generates a branch name from the task ID.

#### Task Execution Directory

Each plan creates a task directory at `.newton/tasks/{task_id}/` with:

- **`input/spec.md`** - The original plan specification
- **`state/`** - Iteration state, context, promise files, and control file
- **`output/`** - Task execution results and logs

The task ID is derived from the plan filename (sanitized to remove special characters).

#### Plan lifecycle

1. Plan file is created in `.newton/plan/{project_id}/todo/`
2. `newton batch {project_id}` picks it up for processing
3. Plan content is copied to `.newton/tasks/{task_id}/input/spec.md`
4. Newton runs the workflow from `workflow_file` in your `.conf`, passing the spec path and workspace in the trigger payload (same as running that workflow yourself with `newton run`)
5. If the workflow completes successfully, the plan moves to `completed/`
6. If the workflow errors, the plan moves to `failed/`

Re-queuing a plan (moving it back to `todo/`) reuses the same task ID and task directories under `.newton/tasks/`.

## Logging

Newton sends log output to files and sometimes to the terminal depending on how you invoke the CLI:

- **Interactive commands** (for example `newton run`, `newton init`): console output defaults to `stderr` for normal messages.
- **`newton batch`**: quiet terminal by default; inspect the log file when troubleshooting.
- **`NEWTON_REMOTE_AGENT=1`**: keeps file logging on and avoids spamming the agent console.

All commands write to `<workspace>/.newton/logs/newton.log` when a workspace is detected, or to `$HOME/.newton/logs/newton.log` as a fallback. The directory is created automatically when missing, and paths provided via config are normalized to avoid accidental traversal.

Newton looks for an optional `.newton/config/logging.toml` file and applies the settings only when the file exists. Invalid values (such as a malformed OpenTelemetry endpoint) fail fast with a clear message, while the absence of the config file is not treated as an error.

### Log configuration

You can tune logging with:

1. Optional `.newton/config/logging.toml` (keys such as `logging.log_dir`, `logging.default_level`, `logging.enable_file`, `logging.console_output`, and `logging.opentelemetry.*` when present).
2. Environment variables, including `RUST_LOG` for tracing filter/verbosity only, `NEWTON_REMOTE_AGENT`, and `OTEL_EXPORTER_OTLP_ENDPOINT` for OpenTelemetry export when configured.
3. Built-in defaults when no file is present: typically `info`, file logging on, console on for local interactive use, telemetry off unless you enable it.

OpenTelemetry export runs only when a valid endpoint is set in config or via `OTEL_EXPORTER_OTLP_ENDPOINT`. `RUST_LOG` overrides the tracing filter level when set; it does not change the log directory.

### Changing the log location

By default Newton writes the tracing log to `<workspace>/.newton/logs/newton.log`. To redirect it to a different directory for a single invocation, pass `--log-dir`:

```bash
newton --log-dir /tmp/newton-logs run my-workflow.yaml
newton --log-dir /var/log/newton batch --once
```

Relative log paths are normalized under the workspace `.newton` directory, or under `$HOME/.newton` when no workspace is detected. You can also set `logging.log_dir` in `.newton/config/logging.toml` to change the default permanently for a workspace.

Log directory precedence is: `--log-dir` for the current invocation, then `logging.log_dir` from `.newton/config/logging.toml`, then the workspace default. Use `RUST_LOG` separately when you only want more or less verbose tracing output.

### Reviewing execution history

Newton records each workflow run to `.newton/state/workflows/<execution-id>/`. Use `newton log` subcommands to inspect past runs:

```bash
# List recent runs in the current workspace (newest first)
newton runs list

# Limit output to the last 5 runs
newton runs list --last 5

# Show task-by-task replay for a specific run
newton runs show <RUN_ID>

# Filter to a single task and show resolved parameters
newton runs show <RUN_ID> --task my-task-id --verbose

# Output as JSON (for scripting)
newton runs list --json
newton runs show <RUN_ID> --json
```

When a task fails, Newton prints a hint to stdout:

```
newton: task failed run_id=<UUID> task_id=<TASK_ID> inspect: newton runs show <UUID> --task <TASK_ID>
```

If you invoke `newton runs show` from a directory other than the workspace root, pass `--workspace <path>` so Newton can locate the execution state (e.g. `newton runs show <UUID> --task <TASK_ID> --workspace /path/to/workspace`).

### Troubleshooting logging

- To temporarily force console logging for debugging, set `logging.console_output = "stderr"` or add `logging.console_output = "stdout"` and rely on the file sink for production runs.
- Setting `NEWTON_REMOTE_AGENT=1` switches the context to `RemoteAgent`, which guarantees file logging remains active even when the console is disabled, making it ideal for remote workers or batch troubleshooting.

### `init <workspace-path>` (same as `init [workspace-path]`)

See **Commands Reference → `init [workspace-path]`** above. The command installs the bundled or selected template, creates the usual `.newton/` directories, and writes `configs/default.conf`.

## Advanced Usage

### Time and Concurrency Limits

Configure execution limits for workflows:

```bash
# Set a wall-clock time limit (30 minutes)
newton run workflow.yaml --timeout 1800

# Limit concurrent task execution
newton run workflow.yaml --parallel-limit 4

# Combined: time limit and concurrency
newton run workflow.yaml --timeout 3600 --parallel-limit 2
```

### Git and plans

Plan files are Markdown and may include YAML frontmatter (for example a `branch` field) for **your** workflows and scripts to read. Newton batch copies the plan into `.newton/tasks/<task_id>/input/spec.md` and runs the configured workflow; any git checkout, commit, or PR steps belong in workflow tasks or helper scripts you invoke from the workflow.

## Configuration

### Workspace Structure

Newton expects the following workspace structure:

```
workspace/
├── .newton/                # Newton workspace directory
│   ├── workflows/          # Workflow YAML (after init from default template)
│   ├── scripts/            # Optional helper shell scripts from template
│   ├── configs/            # Batch configuration files (*.conf)
│   │   └── default.conf
│   ├── plan/               # Batch processing plans
│   │   └── {project_id}/
│   │       ├── todo/       # Pending work items
│   │       ├── completed/  # Successfully processed
│   │       ├── failed/     # Failed items
│   │       └── draft/      # Draft items
│   ├── tasks/              # Task execution state
│   │   └── {task_id}/
│   │       ├── input/      # Task specification
│   │       ├── state/      # Task state files
│   │       └── output/     # Task results
│   ├── state/              # Execution state
│   ├── logs/               # Execution logs
│   └── artifacts/          # Generated artifacts
└── (project files)
```

### Workflows and shell tasks

What your commands and agents see at runtime is defined by your workflow YAML (for example `CommandOperator` and `AgentOperator` tasks) and any wrapper scripts you call from there. Use `.newton/tasks/<task_id>/` under the project for per-plan inputs and state when using batch.

### Batch configuration file (`.newton/configs/*.conf`)

Use simple `key=value` lines. For `newton batch`, set:

```conf
# Required for batch
project_root=/path/to/project
workflow_file=path/to/workflow.yaml

# Optional: kept in default.conf after init for templates and tooling; ignored by batch
coding_model=zai-coding-plan/glm-4.7
```

- **`project_root`**: Your project directory; must already contain a `.newton` folder (required for batch).
- **`workflow_file`**: Workflow YAML to run for each queued plan. Relative paths are resolved against `project_root` first, then the workspace root (required for batch).

Any other keys in that file are ignored by `newton batch` unless documented for a different command.

### Resource Limits

Configure execution limits for workflow runs:

```bash
--timeout N    Wall-clock time limit in seconds
--parallel-limit N      Maximum number of tasks to run concurrently
```

## Output, logs, and artifacts

Each workflow run writes checkpoints, task output, and artifacts under your workspace, typically under `.newton/state/` and `.newton/artifacts/` (exact layout depends on the workflow and operators you use). Use `newton run ... --verbose` to print task stdout/stderr to the terminal after each task. Log files are described in **Logging** above.

## Development

Newton's CLI is wired through the [`cli-framework`](https://github.com/aroff/cli-framework)
crate.  All commands are declared once in
`crates/cli/src/cli/framework_setup.rs` (`build_app`); see
[`crates/cli/README.md`](crates/cli/README.md) for the per-command
metadata contract (`summary` / `syntax` / `category`) and the
operational + `ask` command surface added in issue #231.

When adding or renaming a command, update both `framework_setup.rs` and
the `REGISTERED_COMMAND_IDS` constant — the integration tests in
`crates/cli/tests/integration/test_command_metadata.rs` enforce that
every registered command carries valid metadata.  See the cli-framework
skill for upstream `CommandSpec` / `ArgSpec` reference.

## License

See LICENSE file for details.

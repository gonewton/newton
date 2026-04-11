# Newton

**Newton** is a workflow-first CLI for deterministic automation and orchestration.
- A deterministic workflow runner with linting, explain output, checkpointing, artifacts, goal gates, terminal tasks, and completion policy controls.

## What is Newton?

Newton is a **workflow-first** tool for running structured, repeatable automation: you describe steps in YAML (shell commands, agents, human approvals, branching, and checks), and the CLI runs them with clear completion rules, checkpoints, and artifacts. It fits agent-assisted coding, release checklists, and other tasks where you want a defined graph instead of ad hoc scripts.

You can still think in terms of **evaluate → advise → act** when designing workflows (measure, decide, apply), but the unit of execution is always the workflow file you run with `newton run` or `newton batch`.

## Workflow Graph Capabilities

Newton includes a production workflow runner with YAML-defined tasks and deterministic execution semantics:

- Workflow commands: `newton run|lint|validate|dot|explain|resume|checkpoints|artifacts|webhook`
- Safety checks: workflow lint, early validation of expressions, guarded shell usage, reachability checks
- Deterministic completion: goal gates, terminal tasks, explicit completion policy, stable error codes
- Runtime durability: checkpoint persistence, resume support, artifact routing/cleanup, execution warnings
- Authoring support: macros, `include_if` filtering, `{{ ... }}` interpolation, and `$expr` evaluation

### Built-in Workflow Operators

| Operator | Purpose |
|---|---|
| `NoOpOperator` | Pass-through step; useful for routing and branching |
| `CommandOperator` | Run shell commands; captures stdout/stderr as JSON output |
| `SetContextOperator` | Deep-merge a patch object into the global workflow context |
| `ReadControlFileOperator` | Read and parse a JSON file from a path resolved at runtime |
| `AssertCompletedOperator` | Assert that a set of task IDs have completed before proceeding |
| `HumanApprovalOperator` | Pause for a boolean approve/reject decision from a human operator |
| `HumanDecisionOperator` | Pause for a multiple-choice selection from a human operator |

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

- **Required**: The Newton CLI itself, installed via the package instructions above. Once the CLI is available, `newton init .` installs the workspace template for you.
- **Optional**: Git for working with version control, hooks, and batch workflows.
- If `newton init .` cannot complete (missing template, network issues, or template source errors), check your connectivity and that the configured template source is reachable.

## Quick Start

Follow the **Setting up a new project** flow below to go from a blank directory to `newton run` with the default templates and tooling.

### Quick setup: run a simple coding project

1. Create a project directory and `cd` into it.
2. Run `newton init .` to scaffold the workspace.
3. Run a workflow from the template (optional input file as second positional):
   ```bash
   newton run .newton/workflows/develop.yaml --workspace .
   ```

### Setting up a new project

1. Create a project directory and `cd` into it.
2. Run `newton init .` to scaffold `.newton/` (layout, template files such as workflows and helper scripts, and `.newton/configs/default.conf`).
3. Edit `.newton/configs/default.conf`: set `workflow_file` to the workflow YAML `newton batch` should run (see comment in that file). Add a `<project_id>.conf` copy or symlink if you use batch with a non-default id.
4. Run a workflow explicitly, for example: `newton run .newton/workflows/develop.yaml --workspace .` (pick the YAML that matches your template).

For an existing repository, run `newton init .` at the repo root instead of creating a new directory.


### Verify your setup

- Before your first run, confirm the CLI is installed by checking `newton --version`.

### CLI Version & Help

```bash
newton --version
newton 0.5.33

$ newton --help
newton 0.5.33
Newton CLI for optimization and workflow automation

Usage: newton <COMMAND>
```

The help output now includes the same version banner at the top, so you can confirm which release is installed even when scanning command descriptions.

## Commands Reference

### `run <workflow.yaml>`

Execute a workflow graph defined in YAML.

**Options:**
- `--workspace <PATH>`: Workspace root directory (default: current directory)
- `--arg KEY=VALUE`: Merge key into triggers.payload (repeatable)
- `--set KEY=VALUE`: Merge key into workflow context at runtime (repeatable)
- `--max-time-seconds N`: Wall-clock time limit override in seconds
- `--parallel-limit N`: Runtime override for bounded task concurrency
- `--verbose`: Print task stdout/stderr to terminal after each task completes
- `--file <PATH>`: Path to workflow YAML file (alternative to positional argument)
- `--trigger-json <PATH>`: Load JSON object as base trigger payload before --arg overrides

**Examples:**
```bash
# Run with default settings
newton run workflow.yaml

# With workspace and trigger data
newton run workflow.yaml --workspace ./output --arg key=value

# Multiple arguments
newton run workflow.yaml --arg env=prod --arg version=1.2.3

# With time limit
newton run workflow.yaml --max-time-seconds 3600
```

### `init [workspace-path]`

Create the `.newton` workspace layout, install the default Newton workspace template, and write `.newton/configs/default.conf` with `project_root` (absolute path to the directory you initialized), a default `coding_model` (typically `zai-coding-plan/glm-4.7`), and commented guidance for `workflow_file` (set this when using `newton batch`).

**Options:**
- `--template-source <SOURCE>`: Optional template locator (default: `gonewton/newton-templates`). If the value is a path on disk, that template is used; otherwise the built-in template shipped with the CLI is used.

**Examples:**
```bash
newton init .
newton init /path/to/project
```

### `batch <project_id>`

Process queued plan files for a project. `newton batch` discovers the workspace root by walking up from the current directory (or use `--workspace PATH`) until it finds `.newton`. It reads `.newton/configs/<project_id>.conf`, which **must** include `project_root` and `workflow_file` (path to a workflow YAML, resolved relative to `project_root` or the workspace root). Queued items live under `.newton/plan/<project_id>/todo/`. For each plan, Newton runs that workflow the same way as `newton run` on that YAML.

- `--workspace PATH`: Override workspace discovery.
- `--once`: Process one todo file then exit.
- `--sleep SECONDS`: Poll interval when the queue is empty (default 60).

Plan files move from `todo` to `completed` when the workflow succeeds, or to `failed` when it errors.

Other keys in `.conf` files are ignored by `newton batch` unless documented for another command.

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

- **`newton monitor`**: logs go to the log file; the TUI is not mixed with debug log lines by default.
- **Interactive commands** (for example `newton run`, `newton init`): console output defaults to `stderr` for normal messages.
- **`newton batch`**: quiet terminal by default; inspect the log file when troubleshooting.
- **`NEWTON_REMOTE_AGENT=1`**: keeps file logging on and avoids spamming the agent console (does not apply to `monitor`).

All commands write to `<workspace>/.newton/logs/newton.log` when a workspace is detected, or to `$HOME/.newton/logs/newton.log` as a fallback. The directory is created automatically when missing, and paths provided via config are normalized to avoid accidental traversal.

Newton looks for an optional `.newton/config/logging.toml` file and applies the settings only when the file exists. Invalid values (such as a malformed OpenTelemetry endpoint) fail fast with a clear message, while the absence of the config file is not treated as an error.

### Log configuration

You can tune logging with:

1. Optional `.newton/config/logging.toml` (keys such as `logging.log_dir`, `logging.default_level`, `logging.enable_file`, `logging.console_output`, and `logging.opentelemetry.*` when present).
2. Environment variables, including `RUST_LOG` (verbose Newton messages), `NEWTON_REMOTE_AGENT`, and `OTEL_EXPORTER_OTLP_ENDPOINT` for OpenTelemetry export when configured.
3. Built-in defaults when no file is present: typically `info`, file logging on, console on for local interactive use, telemetry off unless you enable it.

OpenTelemetry export runs only when a valid endpoint is set in config or via `OTEL_EXPORTER_OTLP_ENDPOINT`. `RUST_LOG` overrides the default level when set.

### Troubleshooting TUI/logging conflicts

- If `newton monitor` shows garbled output when you run `RUST_LOG=debug`, confirm no console sink is configured (`console_output` defaults to `none` in the TUI context) and inspect `<workspace>/.newton/logs/newton.log` for the emitted events.
- To temporarily force console logging for debugging, set `logging.console_output = "stderr"` or add `logging.console_output = "stdout"` and rely on the file sink for production runs.
- Setting `NEWTON_REMOTE_AGENT=1` switches the context to `RemoteAgent`, which guarantees file logging remains active even when the console is disabled, making it ideal for remote workers or batch troubleshooting.

### `monitor`

Stream live ailoop channels for every project/branch in the workspace via a terminal UI that highlights blocking questions and authorizations, keeps a queue of pending prompts, lets you answer/approve/deny directly in the terminal, and provides filtering (`/`), layout toggle (`V`), queue tab (`Q`), and help (`?`).

**Behavior:**
`newton monitor` walks up from the current directory to find the workspace root containing `.newton`, then reads `ailoop_server_http_url` and `ailoop_server_ws_url` from the first `.newton/configs/*.conf` file that defines both keys (alphabetically) or from `.newton/configs/monitor.conf` when present. It connects to those HTTP and WebSocket endpoints, loads recent messages per channel, and opens a full-screen terminal UI with stream views, a queue for pending prompts, filtering, and keyboard shortcuts (see `?` in the UI for help).

**Options:**
- `--http-url <URL>`: Override the HTTP base URL for this session.
- `--ws-url <URL>`: Override the WebSocket URL for this session.

**Example:**
```bash
newton monitor
```

**Setup with ailoop:**

`newton monitor` works with [ailoop](https://github.com/goailoop/ailoop), a human-in-the-loop messaging server for AI agents. To use newton monitor:

1. **Install ailoop** (for example with Homebrew: `brew install ailoop`, or follow the [ailoop installation docs](https://github.com/goailoop/ailoop)).

2. **Start the ailoop server:**
   ```bash
   ailoop serve
   # Default: WebSocket on ws://127.0.0.1:8080, HTTP API on http://127.0.0.1:8081
   ```

3. **Start newton monitor in another terminal:**
   ```bash
   newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080
   ```

4. **Send messages from agents or other terminals:**
   ```bash
   # Send a notification
   ailoop say "Task completed successfully" --server ws://127.0.0.1:8080 --channel myproject

   # Ask a question
   ailoop ask "Should I proceed with deployment?" --server ws://127.0.0.1:8080 --channel myproject

   # Request authorization
   ailoop authorize "Push to production branch" --server ws://127.0.0.1:8080 --channel myproject
   ```

Messages will appear in the newton monitor UI in real-time. Interactive messages (questions, authorizations) appear in the queue panel where you can respond directly.

**Configuration:**

To avoid passing URLs each time, create `.newton/configs/monitor.conf`:
```
ailoop_server_http_url=http://127.0.0.1:8081
ailoop_server_ws_url=ws://127.0.0.1:8080
```

Then simply run:
```bash
newton monitor
```

### `init <workspace-path>` (same as `init [workspace-path]`)

See **Commands Reference → `init [workspace-path]`** above. The command installs the bundled or selected template, creates the usual `.newton/` directories, and writes `configs/default.conf`.

## Advanced Usage

### Time and Concurrency Limits

Configure execution limits for workflows:

```bash
# Set a wall-clock time limit (30 minutes)
newton run workflow.yaml --max-time-seconds 1800

# Limit concurrent task execution
newton run workflow.yaml --parallel-limit 4

# Combined: time limit and concurrency
newton run workflow.yaml --max-time-seconds 3600 --parallel-limit 2
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
workflow_file=.newton/workflows/develop.yaml

# Optional: kept in default.conf after init for templates and tooling; ignored by batch
coding_model=zai-coding-plan/glm-4.7
```

- **`project_root`**: Your project directory; must already contain a `.newton` folder (required for batch).
- **`workflow_file`**: Workflow YAML to run for each queued plan. Relative paths are resolved against `project_root` first, then the workspace root (required for batch).

Any other keys in that file are ignored by `newton batch` unless documented for a different command.

### Resource Limits

Configure execution limits for workflow runs:

```bash
--max-time-seconds N    Wall-clock time limit in seconds
--parallel-limit N      Maximum number of tasks to run concurrently
```

## Output, logs, and artifacts

Each workflow run writes checkpoints, task output, and artifacts under your workspace, typically under `.newton/state/` and `.newton/artifacts/` (exact layout depends on the workflow and operators you use). Use `newton run ... --verbose` to print task stdout/stderr to the terminal after each task. Log files are described in **Logging** above.

## Troubleshooting

### Monitor Issues

**Monitor not receiving messages:**

1. **Verify ailoop server is running:**
   ```bash
   # Check if ailoop is listening on the correct ports
   lsof -i :8080 -i :8081
   ```

2. **Verbose Newton logging:**
   ```bash
   RUST_LOG=newton=debug,info newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080
   ```
   Then inspect `<workspace>/.newton/logs/newton.log` for connection or parse errors.

3. **Test ailoop server directly:**
   ```bash
   # In one terminal
   ailoop serve

   # In another terminal, test sending a message
   ailoop say "Test message" --server ws://127.0.0.1:8080 --channel test
   ```

4. **Verify message format:**
   Messages sent to ailoop must match the expected format:
   ```json
   {
     "id": "<uuid>",
     "channel": "channel-name",
     "sender_type": "AGENT",
     "content": {
       "type": "notification",
       "text": "Message text",
       "priority": "normal"
     },
     "timestamp": "2024-01-15T10:00:00Z"
   }
   ```

**Common errors in ailoop serve:**

- `Failed to parse message: missing field 'sender_type'` - Message is missing required fields
- `unknown variant 'agent', expected 'AGENT' or 'HUMAN'` - sender_type must be uppercase
- `expected struct Message` - Message structure is incorrect

**Configuration issues:**

If monitor can't find the ailoop URLs, ensure you have either:
- Command-line flags: `--http-url` and `--ws-url`
- Or a config file at `.newton/configs/monitor.conf` with:
  ```
  ailoop_server_http_url=http://127.0.0.1:8081
  ailoop_server_ws_url=ws://127.0.0.1:8080
  ```

## License

See LICENSE file for details.

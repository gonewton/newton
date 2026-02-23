# Newton

**Newton** is a CLI for iterative optimization and workflow automation. It supports both:
- The classic evaluator-advisor-executor loop (`newton run`, `newton step`, `newton batch`)
- A deterministic workflow-graph runner (`newton workflow ...`) with linting, explain output, checkpointing, artifacts, goal gates, terminal tasks, and completion policy controls.

## What is Newton?

Newton is an iterative optimization framework for agentic AI goals that benefit from structured feedback and controlled execution. The classic loop orchestrates three phases:

- **Evaluator**: Assesses the current state/solution and provides quality metrics
- **Advisor**: Generates improvement recommendations based on evaluation
- **Executor**: Implements the recommended changes to improve the solution

This evaluation-advice-execution loop continues until goals are met or iteration limits are reached.

Instead of just trying the same thing over and over and hoping it gets better, this kind of loop pauses to check how things are going, think about what could improve, and then make targeted changes. Each round learns from the last, so progress is more guided than random. It also keeps track of what worked best so far, which is helpful when goals involve trade-offs or gradual improvements rather than a simple yes/no result. Overall, it feels less like “try again” and more like “let’s see what happened and do a bit better next time,” which makes it a good fit for a wide range of problems.

## Workflow Graph Capabilities

Newton includes a production workflow runner with YAML-defined tasks and deterministic execution semantics:

- Workflow commands: `newton workflow run|lint|validate|dot|explain|resume|checkpoints|artifacts|webhook`
- Safety checks: lint rules, expression precompile validation, shell opt-in, reachability analysis
- Deterministic completion: goal gates, terminal tasks, explicit completion policy, stable error codes
- Runtime durability: checkpoint persistence, resume support, artifact routing/cleanup, execution warnings
- Authoring ergonomics: transform pipeline with macro expansion, `include_if` filtering, `{{ ... }}` interpolation, and `$expr` evaluation

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
3. Run the loop with an inline goal (no `GOAL.md` needed):
   ```bash
   newton run . --goal "Add a README.md that describes this project in one paragraph."
   ```
4. Use the execution ID printed by `newton run` with `newton status <execution-id>` to inspect progress.

For a project with a persistent goal file or custom evaluator/advisor/executor scripts, follow **Setting up a new project** below.

### Setting up a new project

1. Create a project directory and `cd` into it.
2. Run `newton init .` to scaffold `.newton/`, install the template under `.newton/scripts/`, write `.newton/configs/default.conf`, and prompt the template to add `GOAL.md` if it was missing.
3. Optionally edit `GOAL.md` after initialization so it reflects your real goal.
4. Run `newton run` in that directory—`run` uses the `.newton/scripts/` toolchain by default, so no additional paths are required.
5. Use `newton status`, `newton report`, and `newton error` with the returned execution ID to inspect progress and failures.

For an existing repository, run `newton init .` at the repo root instead of creating a new directory.

To swap in custom evaluators, advisors, or executors, either pass `--evaluator`, `--advisor`, and `--executor` to `newton run` or replace the scripts under `.newton/scripts/` after initialization; see **Advanced Usage → Custom Tool Configuration** for details.

### Verify your setup

- Before your first run, confirm the CLI is installed by checking `newton --version`.
- After `newton init .`, list the layout (`ls .newton`) or run `newton step .` once to verify `.newton/scripts/` and `.newton/configs/default.conf` exist.
- Optionally run `newton step .` to exercise the default template before starting the full loop.

### CLI Version & Help

```bash
newton --version
newton 0.3.8

$ newton --help
newton 0.3.8
Newton CLI for optimization and workflow automation

Usage: newton <COMMAND>
```

The help output now includes the same version banner at the top, so you can confirm which release is installed even when scanning command descriptions.

## Repository

This repository includes a Repomix pack (`repomix-output.xml`) for contributors who want AI-assisted analysis or review assistance.

## Commands Reference

### `run [workspace-path]`

Start optimization loop for a workspace.

If the workspace path is omitted the command runs in the current directory. After `newton init .` the `.newton/scripts` toolchain is installed automatically, so you can rely on the default `evaluator.sh`, `advisor.sh`, and `executor.sh` without passing strict-mode overrides.

**Options:**
- `--max-iterations N`: Maximum iterations before stopping
- `--timeout N`: Maximum time in seconds before stopping
- `--tool-timeout N`: Timeout per tool execution in seconds
- `--evaluator <command>`: Custom evaluator command
- `--advisor <command>`: Custom advisor command
- `--executor <command>`: Custom executor command
- `--strict-mode`: Enable strict validation mode
- `--goal <TEXT>`: Inline goal description written to `.newton/state/goal.txt` and exported as `NEWTON_GOAL_FILE` (directories are created automatically when needed)
- `--goal-file <FILE>`: Use an existing goal file instead of writing from CLI text (`NEWTON_GOAL_FILE` is still populated).

Passing empty evaluator/advisor/executor commands now fails fast with `TOOL-002` (`command must not be empty`). Provide valid tool invocations (or omit the flag) so the orchestrator can launch real scripts.

**Examples:**
```bash
# Run with default settings
newton run .

# Run with custom timeouts
newton run . --max-iterations 100 --timeout 3600

# Use custom tools
newton run . --evaluator ./.newton/scripts/my_evaluator.sh
```

### `init [workspace-path]`

Create the `.newton` workspace layout, install the default Newton template via `aikit-sdk`, and write `.newton/configs/default.conf` with `project_root=.`, `coding_agent=opencode`, and the default `zai-coding-plan/glm-4.7` model.

**Options:**
- `--template-source <SOURCE>`: Optional template locator (default: `gonewton/newton-templates`). Paths that exist on disk are copied directly, otherwise the built-in template is used.

**Examples:**
```bash
newton init .
newton init /path/to/project
```

### `batch <project_id>`

Process queued plan files for a project straight from the CLI. `newton batch` discovers the workspace root by walking up from the current directory (or use `--workspace PATH`) until it finds `.newton`. It expects `.newton/configs/<project_id>.conf` to contain at least `project_root`, `coding_agent`, and `coding_model`, and `.newton/plan/<project_id>/todo` to house queued plan files. Each plan is copied into `project_root/.newton/tasks/<task_id>/input/spec.md` and fed to the regular `newton run` flow.

- `--workspace PATH`: Override workspace discovery.
- `--once`: Process one todo file then exit.
- `--sleep SECONDS`: Poll interval when the queue is empty (default 60).

Plan files move from `todo` to `completed` only after a successful run, and the same task ID is reused when a plan is re-queued. Batch also sets `CODING_AGENT`, `CODING_AGENT_MODEL`, `NEWTON_EXECUTOR_CODING_AGENT`, and `NEWTON_EXECUTOR_CODING_AGENT_MODEL` based on the `.conf` so the project honors those overrides.

You can add `post_success_script` and `post_fail_script` entries to `.newton/configs/<project_id>.conf`. Each value is run with `sh -c "<value>"` from the project root. `post_success_script` executes only after a successful `newton run` and keeps the plan under `completed/` when it exits `0`; any non-zero exit code flips the plan into the new `.newton/plan/<project_id>/failed/` directory (the target is overwritten when necessary). `post_fail_script` runs when `newton run` fails, its exit code is ignored, and the plan ends up in `failed/` as well. Both scripts receive the batch environment plus `NEWTON_GOAL_FILE`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`, `NEWTON_PROJECT_ROOT`, and `NEWTON_RESULT=success|failure`.

Batch configs now support additional optional keys for parity with `start.sh`/`loop.sh`:

| Key | Description |
| --- | --- |
| `evaluator_cmd`, `advisor_cmd`, `executor_cmd` | Override the tool invocations. When omitted, batch derives defaults that match the workspace layout created by `newton init` (`project_root/.newton/scripts/{evaluator,advisor}.sh` and `workspace_root/.newton/scripts/executor.sh`). |
| `pre_run_script` | Runs once before `newton run` (e.g., `.newton/scripts/pre-run.sh`). |
| `resume` | `true`/`1` keeps the task and project state directories intact; when false the directories are wiped before the run. |
| `max_iterations`, `max_time` | Optional limits that mirror the values passed to `newton run`. Without a control file signal, hitting either limit counts as failure. |
| `verbose` | Passes `--verbose` through to the run so tool stdout/stderr is rendered. |
| `control_file` | The filename (default `newton_control.json`) stored inside the task state directory; evaluators must write `{"done": true}` to `NEWTON_CONTROL_FILE` in that folder to signal success. |

Batch exposes the following environment variables for pre-run hooks, tool runs, and post hooks: `NEWTON_STATE_DIR`, `NEWTON_WS_ROOT`, `NEWTON_CODER_CMD`, `NEWTON_CONTROL_FILE`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`, `CODING_AGENT`, and `CODING_AGENT_MODEL`. Pre-run scripts also see `NEWTON_PROJECT_ROOT`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`, `NEWTON_GOAL_FILE`, and `NEWTON_RESUME`. Post hooks additionally receive `NEWTON_RESULT`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`, `NEWTON_STATE_DIR`, and `NEWTON_CONTROL_FILE` so they can reference the same artifacts without re-reading the plan.

The feature branch name is derived from the plan frontmatter (`branch: feature/foo`) when present; otherwise batch uses `feature/<task_id>` with underscores replaced by dashes. This value populates `NEWTON_BRANCH_NAME` everywhere so hooks can operate on the right Git branch without re-parsing the spec.

Success is gatekept by the control file that lives under `NEWTON_STATE_DIR`. Evaluator scripts must write `{"done": true}` to `NEWTON_CONTROL_FILE` when the goal is reached and `{"done": false}` (or remove the file) when more iterations are required. The loop stops only when the control file reports `done: true`; every other termination (limits, errors, or missing file) is treated as failure and sends the plan to `failed/`.

Projects can opt into git hooks by adding a `[hooks]` section to `newton.toml`:

```toml
[hooks]
before_run = "git checkout main"
after_run = "git checkout $NEWTON_RESULT"
```

Hook commands always run with `sh -c "<value>"` inside the project root. `before_run` executes before the orchestrator and sees `NEWTON_GOAL_FILE`, `NEWTON_PROJECT_ID`, and `NEWTON_TASK_ID` if those variables exist. `after_run` always runs (even on failure) and is passed `NEWTON_RESULT=success|failure` plus `NEWTON_EXECUTION_ID` when available.

Workspace discovery and the `.conf` parser in `core/batch_config` are shared with the upcoming monitor so logic is not duplicated.

### Plan-Based Batch Processing Architecture

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

#### Plan Lifecycle

1. Plan file is created in `.newton/plan/{project_id}/todo/`
2. `newton batch {project_id}` picks it up for processing
3. Plan content is copied to `.newton/tasks/{task_id}/input/spec.md`
4. `newton run` executes with the plan as the goal
5. On success (control file shows `done: true`), plan moves to `completed/`
6. On failure (limits hit, errors, or `done: false`), plan moves to `failed/`

Re-queuing a plan (moving it back to `todo/`) reuses the same task ID, allowing state to be preserved if `resume=true` is set in the configuration.

## Logging

Newton routing now keeps logs deterministic across every command by mapping the CLI command and environment to an execution context. Contexts are:

- **TUI**: `newton monitor` with no console output (logs persist to file only).
- **LocalDev**: interactive commands such as `run`, `step`, `status`, `report`, `error`, and `init`; console output defaults to `stderr`.
- **Batch**: `newton batch` runs in quiet mode, writing only to files unless overridden.
- **RemoteAgent**: any command run with `NEWTON_REMOTE_AGENT=1` (except `monitor`) – file logging stays enabled and console output remains disabled.

All commands write to `<workspace>/.newton/logs/newton.log` when a workspace is detected, or to `$HOME/.newton/logs/newton.log` as a fallback. The directory is created automatically when missing, and paths provided via config are normalized to avoid accidental traversal.

Newton looks for an optional `.newton/config/logging.toml` file and applies the settings only when the file exists. Invalid values (such as a malformed OpenTelemetry endpoint) fail fast with a clear message, while the absence of the config file is not treated as an error.

### Configuration precedence

Logging behavior can be customized via:

1. CLI flags (future-proof – none exist yet for this feature).
2. Environment variables such as `RUST_LOG`, `NEWTON_REMOTE_AGENT`, and `OTEL_EXPORTER_OTLP_ENDPOINT`.
3. `.newton/config/logging.toml` using keys like `logging.log_dir`, `logging.default_level`, `logging.enable_file`, `logging.console_output`, and the `logging.opentelemetry.*` table.
4. Built-in defaults (`info` level, file enabled, console only for local dev, no telemetry).

OpenTelemetry exports only activate when either `logging.opentelemetry.endpoint` or `OTEL_EXPORTER_OTLP_ENDPOINT` provides a valid URL, and the endpoint string is validated before use. The framework always honors `RUST_LOG` as the highest-priority level filter so you can drive verbosity without touching the config file.

### Troubleshooting TUI/logging conflicts

- If `newton monitor` shows garbled output when you run `RUST_LOG=debug`, confirm no console sink is configured (`console_output` defaults to `none` in the TUI context) and inspect `<workspace>/.newton/logs/newton.log` for the emitted events.
- To temporarily force console logging for debugging, set `logging.console_output = "stderr"` or add `logging.console_output = "stdout"` and rely on the file sink for production runs.
- Setting `NEWTON_REMOTE_AGENT=1` switches the context to `RemoteAgent`, which guarantees file logging remains active even when the console is disabled, making it ideal for remote workers or batch troubleshooting.
### `step <workspace-path>`

Execute a single evaluation-advice-execution iteration.

**Options:**
- `--tool-timeout N`: Timeout per tool execution in seconds
- `--strict-mode`: Enable strict validation mode

**Example:**
```bash
newton step .
```

### `status <execution-id>`

Check current status of an optimization run.

**Options:**
- `--format <format>`: Output format (text, json)
- `--verbose`: Show detailed execution information

**Example:**
```bash
newton status abc-123 --format json
```

**Output:**
- Current iteration count
- Last evaluation score
- Overall progress toward goals
- Execution status (running, completed, failed)
- Time elapsed

### `report <execution-id>`

Generate a comprehensive execution report.

**Options:**
- `--format <format>`: Output format (text, json)
- `--include-stats`: Include performance statistics

**Examples:**
```bash
# Generate text report
newton report abc-123

# Generate JSON report for programmatic access
newton report abc-123 --format json

# Generate report with statistics
newton report abc-123 --include-stats
```

**Report Contents:**
- Overall execution summary
- Iteration-by-iteration progress
- Tool execution logs
- Final evaluation metrics
- Performance statistics
- Error messages (if any)

### `error <execution-id>`

Debug execution errors with detailed information.

**Options:**
- `--verbose`: Show detailed stack traces and logs
- `--show-artifacts`: Include generated artifacts in output

**Example:**
```bash
newton error abc-123 --verbose
```

**Diagnostic Information:**
- Error type and location
- Tool execution failures
- Workspace validation errors
- Generated artifacts
- Execution logs
- Recovery recommendations

### `monitor`

Stream live ailoop channels for every project/branch in the workspace via a terminal UI that highlights blocking questions and authorizations, keeps a queue of pending prompts, lets you answer/approve/deny directly in the terminal, and provides filtering (`/`), layout toggle (`V`), queue tab (`Q`), and help (`?`).

**Behavior:**
`newton monitor` walks up from the current directory to find the workspace root containing `.newton`, then reads `ailoop_server_http_url` and `ailoop_server_ws_url` from the first `.newton/configs/*.conf` file that exposes both keys (alphabetically) or from `.newton/configs/monitor.conf` when present. It connects to the configured HTTP and WebSocket endpoints, backfills up to 50 messages per channel, subscribes to channels, and renders a ratatui interface with tiles/list stream views, a 30% queue panel, a filter status line, and optional queue-only mode.

**Options:**
- `--http-url <URL>`: Override the HTTP base URL for this session.
- `--ws-url <URL>`: Override the WebSocket URL for this session.

**Example:**
```bash
newton monitor
```

**Setup with ailoop:**

`newton monitor` works with [ailoop](https://github.com/goailoop/ailoop), a human-in-the-loop messaging server for AI agents. To use newton monitor:

1. **Install ailoop:**
   ```bash
   # Using Homebrew
   brew install ailoop

   # Or using cargo
   cargo install ailoop-cli
   ```

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

### `init <workspace-path>`

Bootstrap a workspace from an installed Newton template. See **Quick Start → Setting up a new project** for the minimal flow that gets a fresh directory to `newton run`. `newton init` renders `.newton/scripts`, `.newton/state`, and `newton.toml`, seeds `GOAL.md`, and keeps everything in sync with the template variables (`project_name`, `coding_agent`, `coding_agent_model`, `test_command`, `language`). The command requires `aikit` to be available on `PATH` and at least one template directory under `.newton/templates/` (templates can be installed via `aikit` packages or checked in alongside your projects).

**Options:**
- `--template <NAME>`: Choose a template (default: `basic`). The template name must match a subdirectory under `.newton/templates/`.
- `--name <NAME>`: Override the project name written to `newton.toml` and used in the GOAL stub.
- `--coding-agent <AGENT>`: Specify the coding agent that will be listed in `newton.toml`.
- `--model <MODEL>`: Override the coding agent model in the generated config.
- `--interactive`: Prompt for missing values instead of assuming defaults.
- `--force`: Proceed even if `.newton/` already exists (existing files are overwritten).

**Behavior:**
- Validates that `aikit` is installed (`aikit --version` must succeed); otherwise prints an install hint (`https://aikit.readthedocs.io`) and exits with an error.
- Clears `.newton/state/context.md`, writes fresh `promise.txt`/`executor_prompt.md`/`iteration.txt`, and renders the selected template into `.newton/`.
- Writes `newton.toml` only when it does not already exist, defaults the `project.template` to the template name, and populates `executor.coding_agent`/`coding_agent_model` plus the recommended `test_command`.
- Creates `GOAL.md` with a placeholder goal if it is missing.

**Example:**
```bash
newton init . --template basic --interactive
```

## Advanced Usage

### Custom Tool Configuration

Newton allows you to specify custom commands for each optimization phase:

```bash
newton run . \
  --evaluator "python .newton/scripts/evaluator.py" \
  --advisor "python .newton/scripts/advisor.py" \
  --executor "python .newton/scripts/executor.py"
```

### Timeout Configurations

Configure timeouts at different levels:

```bash
# Overall timeout (30 minutes)
newton run . --timeout 1800

# Per-tool timeout (5 minutes)
newton run . --tool-timeout 300

# Combined approach
newton run . --timeout 3600 --tool-timeout 300
```

### Iteration and Time Limits

```bash
# Run at most 50 iterations
newton run . --max-iterations 50

# Stop after 10 minutes
newton run . --timeout 600

# Stop when either condition is met
newton run . --max-iterations 50 --timeout 600
```

### Strict Mode

Enable strict validation mode for critical operations:

```bash
newton run . --strict-mode
```

Strict mode requires:
- All tools to exit with code 0
- Workspace validation to pass
- Evaluation score to be positive
- No unexpected errors during execution

### Resource Limits and Monitoring

```bash
newton run . \
  --max-iterations 100 \
  --timeout 3600 \
  --tool-timeout 300 \
  --memory-limit 4G
```

Monitor execution in real-time:

```bash
# Watch execution status
newton status <execution-id> --format json --verbose

# Generate periodic reports
newton report <execution-id> --include-stats
```

### Git Integration

Newton includes built-in git integration for batch processing workflows:

#### Automatic Branch Management

Each plan can specify a git branch in its frontmatter:

```markdown
---
branch: feature/add-authentication
---

# Task Description
...
```

**Branch name resolution:**
1. Plan file frontmatter (`branch:` field) - highest priority
2. Auto-generated from task ID (`feature/{task_id}`) with underscores converted to dashes
3. Environment variables (`NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`)

The resolved branch name is exposed via `NEWTON_BRANCH_NAME` environment variable to all tools and hooks.

#### Git Hooks in newton.toml

Configure git operations in your project's `newton.toml`:

```toml
[hooks]
before_run = "git checkout -b $NEWTON_BRANCH_NAME"
after_run = "git add . && git commit -m 'Automated changes' || true"
```

**Hook environment variables:**
- `before_run`: Receives `NEWTON_GOAL_FILE`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`
- `after_run`: Receives all of the above plus `NEWTON_RESULT` (success|failure) and `NEWTON_EXECUTION_ID`

Hooks run with `sh -c "<command>"` from the project root directory.

#### Git Operations in Scripts

Batch mode automatically detects and exposes:
- Current git branch
- Base branch (typically `main` or `master`)
- Both values are available in `NEWTON_BRANCH_NAME` and `NEWTON_BASE_BRANCH`

Your custom scripts can use these for:
- Creating feature branches
- Committing changes during optimization
- Creating pull requests after successful completion
- Rolling back on failure

## Configuration

### Workspace Structure

Newton expects the following workspace structure:

```
workspace/
├── .newton/                # Newton workspace directory
│   ├── scripts/            # Tool scripts
│   │   ├── evaluator.sh
│   │   ├── advisor.sh
│   │   ├── executor.sh
│   │   └── coder.sh        # Optional coder script
│   ├── configs/            # Batch configuration files
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
└── GOAL.md                 # Optimization objectives (optional)
```

### Toolchain Configuration

Each tool script receives environment variables:

**Common Environment Variables:**
- `NEWTON_WORKSPACE_PATH`: Absolute path to workspace root
- `NEWTON_ITERATION`: Current iteration number
- `NEWTON_STATE_DIR`: Path to state directory
- `NEWTON_ARTIFACTS_DIR`: Path to artifacts directory

**Evaluator Environment Variables:**
- `NEWTON_SCORE_FILE`: Path where score must be written
- `NEWTON_EVALUATOR_DIR`: Path for evaluator output files

**Advisor Environment Variables:**
- `NEWTON_ADVISOR_DIR`: Path for advisor recommendations

**Executor Environment Variables:**
- `NEWTON_EXECUTOR_DIR`: Path for executor logs
- `NEWTON_SOLUTION_FILE`: Path to current solution file

### Batch Configuration File (.newton/configs/*.conf)

Newton uses simple key=value configuration files for batch processing:

```conf
# Required settings
project_root=/path/to/project
coding_agent=claude
coding_model=claude-sonnet-4-5

# Optional tool commands
evaluator_cmd=.newton/scripts/evaluator.sh
advisor_cmd=.newton/scripts/advisor.sh
executor_cmd=.newton/scripts/executor.sh
coder_cmd=.newton/scripts/coder.sh

# Hook scripts
pre_run_script=.newton/scripts/pre-run.sh
post_success_script=.newton/scripts/post-success.sh
post_fail_script=.newton/scripts/post-failure.sh

# Execution control
resume=true
max_iterations=10
max_time=3600
verbose=true
control_file=newton_control.json
```

**Configuration Keys:**
- `project_root`: Root directory of your project (required)
- `coding_agent`: AI agent to use, e.g., "claude", "openai" (required)
- `coding_model`: Specific model identifier (required)
- `evaluator_cmd`, `advisor_cmd`, `executor_cmd`, `coder_cmd`: Custom tool commands (optional, defaults to `.newton/scripts/*.sh`)
- `pre_run_script`: Script to run before each task (optional)
- `post_success_script`: Script to run after successful task completion (optional)
- `post_fail_script`: Script to run after task failure (optional)
- `resume`: Whether to resume from previous state - `true`/`1` or `false`/`0` (optional, default: false)
- `max_iterations`: Maximum optimization iterations per task (optional)
- `max_time`: Maximum execution time in seconds (optional)
- `verbose`: Enable verbose logging - `true`/`1` or `false`/`0` (optional, default: false)
- `control_file`: Filename for control file stored in task state directory (optional, default: `newton_control.json`)

### Control Files for Success Determination

Control files provide a mechanism for evaluator scripts to signal task completion in batch processing workflows.

#### How Control Files Work

The control file is a JSON file stored in the task's state directory (`NEWTON_STATE_DIR`) that the evaluator writes to indicate whether the optimization goal has been met:

```json
{
  "done": true
}
```

**File location:** `{task_state_dir}/{control_file_name}`
- Path is available via `NEWTON_CONTROL_FILE` environment variable
- Default filename is `newton_control.json` (configurable via `control_file` config key)

#### Success Criteria

The batch orchestrator determines task success based on the control file:

- **Success:** Control file exists with `"done": true`
- **Failure:** Any of the following:
  - Control file missing
  - Control file has `"done": false`
  - Iteration or time limits reached without `"done": true`
  - Errors during execution

#### Evaluator Script Example

```bash
#!/bin/bash
# evaluator.sh

# Run tests
if npm test; then
  # Tests passed - signal completion
  echo '{"done": true}' > "$NEWTON_CONTROL_FILE"
  echo "All tests passed!" > "$NEWTON_EVALUATOR_DIR/status.md"
else
  # Tests failed - need more iterations
  echo '{"done": false}' > "$NEWTON_CONTROL_FILE"
  echo "Tests failed, needs improvement" > "$NEWTON_EVALUATOR_DIR/status.md"
fi
```

#### Extended Control File Format

You can include additional metadata in the control file:

```json
{
  "done": true,
  "message": "All acceptance criteria met",
  "score": 95,
  "tests_passed": 42,
  "tests_failed": 0
}
```

The orchestrator only checks the `done` field; additional fields are for logging and debugging purposes.

### Hook Scripts

Hook scripts allow you to run custom commands at different stages of the batch processing lifecycle.

#### Configuring Hooks

Add hook script paths to your `.newton/configs/{project_id}.conf`:

```conf
# Pre-run hook: executes before newton run starts
pre_run_script=.newton/scripts/pre-run.sh

# Post-success hook: executes after successful task completion
post_success_script=.newton/scripts/post-success.sh

# Post-failure hook: executes after task failure
post_fail_script=.newton/scripts/post-failure.sh
```

All hooks run with `sh -c "<script_path>"` from the project root directory.

#### Hook Types

**Pre-run Hook (`pre_run_script`)**
- Executes once before `newton run` starts
- Use for: environment setup, dependency installation, test database initialization
- Environment variables available:
  - `NEWTON_PROJECT_ROOT`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`
  - `NEWTON_GOAL_FILE`, `NEWTON_RESUME`
  - All batch environment variables

**Post-success Hook (`post_success_script`)**
- Executes after successful task completion (control file shows `done: true`)
- Exit code determines final plan state:
  - `0` - Plan moves to `completed/`
  - Non-zero - Plan moves to `failed/` (overrides success)
- Use for: deployment, creating pull requests, notifications, cleanup
- Environment variables available:
  - All batch environment variables
  - `NEWTON_RESULT=success`
  - `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`
  - `NEWTON_STATE_DIR`, `NEWTON_CONTROL_FILE`

**Post-failure Hook (`post_fail_script`)**
- Executes after task failure
- Exit code is ignored (plan always moves to `failed/`)
- Use for: rollback, error notifications, cleanup, logging
- Environment variables available:
  - All batch environment variables
  - `NEWTON_RESULT=failure`
  - `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`
  - `NEWTON_STATE_DIR`, `NEWTON_CONTROL_FILE`

#### Example Hook Scripts

**Pre-run hook (setup environment):**
```bash
#!/bin/bash
# .newton/scripts/pre-run.sh

echo "Setting up environment for task $NEWTON_TASK_ID"

# Install dependencies
npm install

# Create feature branch if it doesn't exist
if ! git rev-parse --verify "$NEWTON_BRANCH_NAME" >/dev/null 2>&1; then
  git checkout -b "$NEWTON_BRANCH_NAME"
else
  git checkout "$NEWTON_BRANCH_NAME"
fi
```

**Post-success hook (create PR):**
```bash
#!/bin/bash
# .newton/scripts/post-success.sh

echo "Task completed successfully: $NEWTON_TASK_ID"

# Commit changes
git add .
git commit -m "feat: $NEWTON_TASK_ID" || true

# Push to remote
git push origin "$NEWTON_BRANCH_NAME"

# Create pull request
gh pr create --title "$NEWTON_TASK_ID" --body "Automated implementation" --base "$NEWTON_BASE_BRANCH"
```

**Post-failure hook (cleanup and notify):**
```bash
#!/bin/bash
# .newton/scripts/post-failure.sh

echo "Task failed: $NEWTON_TASK_ID"

# Send notification
curl -X POST https://hooks.slack.com/... \
  -H 'Content-Type: application/json' \
  -d "{\"text\":\"Task $NEWTON_TASK_ID failed\"}"

# Cleanup partial changes
git reset --hard HEAD
```

### Environment Variables Available to Tools

Tools can access Newton's environment variables:

#### Basic Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `NEWTON_WORKSPACE_PATH` | Workspace root directory | `/path/to/workspace` |
| `NEWTON_ITERATION` | Current iteration number | `5` |
| `NEWTON_SCORE_FILE` | Evaluator output file | `/path/to/workspace/.newton/score.txt` |
| `NEWTON_STATE_DIR` | State directory | `/path/to/workspace/.newton/state` |
| `NEWTON_ARTIFACTS_DIR` | Artifacts directory | `/path/to/workspace/.newton/artifacts` |

#### Batch Processing Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `NEWTON_PROJECT_ROOT` | Project root directory | `/path/to/project` |
| `NEWTON_PROJECT_ID` | Current project identifier | `myproject` |
| `NEWTON_TASK_ID` | Current task identifier | `feature-123-abc` |
| `NEWTON_RESUME` | Whether resuming from previous state | `1` or `0` |
| `NEWTON_RESULT` | Result status (in post hooks) | `success` or `failure` |
| `NEWTON_CONTROL_FILE` | Path to control file | `/path/.newton/tasks/abc/state/newton_control.json` |
| `NEWTON_WS_ROOT` | Workspace root path | `/path/to/workspace` |
| `NEWTON_CODER_CMD` | Coder tool command | `.newton/scripts/coder.sh` |
| `NEWTON_BRANCH_NAME` | Git branch name for task | `feature/add-auth` |
| `NEWTON_BASE_BRANCH` | Git base branch | `main` |
| `NEWTON_GOAL_FILE` | Path to goal file | `/path/.newton/state/goal.txt` |
| `CODING_AGENT` | AI agent identifier | `claude` |
| `CODING_AGENT_MODEL` | AI model identifier | `claude-sonnet-4-5` |
| `NEWTON_EXECUTOR_CODING_AGENT` | Executor's coding agent | `claude` |
| `NEWTON_EXECUTOR_CODING_AGENT_MODEL` | Executor's model | `claude-sonnet-4-5` |

### Resource Limits

Configure resource limits to control optimization runs:

```bash
--max-iterations N    Maximum iterations (default: 100)
--timeout N           Maximum time in seconds (default: 3600)
--tool-timeout N      Timeout per tool in seconds (default: 60)
--memory-limit N      Maximum memory per tool (e.g., 4G)
```

## Output and Artifacts

### Generated Artifacts

Newton generates several artifacts during execution:

**Evaluator Outputs:**
- `evaluator_status.md`: Evaluation results and metrics
- `evaluation_score.txt`: Numeric quality score

**Advisor Outputs:**
- `advisor_recommendations.md`: Improvement suggestions
- `recommendations.json`: Machine-readable recommendations

**Executor Outputs:**
- `executor_log.md`: Detailed execution logs
- `changes_applied.md`: List of changes made
- `solution_state.json`: Current solution state

### Execution History

All execution state is persisted in the `.newton/` directory:

```
.newton/
├── state/
│   ├── execution.json      # Execution metadata
│   ├── current_solution.json
│   └── iteration_history.json
├── artifacts/
│   ├── evaluator_status.md
│   ├── advisor_recommendations.md
│   └── executor_log.md
└── logs/
    └── execution.log
```

### Report Formats

Reports can be generated in multiple formats:

**Text Format** (human-readable):
```bash
newton report <execution-id>
```

**JSON Format** (machine-readable):
```bash
newton report <execution-id> --format json
```

**JSON Output Structure:**
```json
{
  "execution_id": "abc-123",
  "status": "completed",
  "iteration": 10,
  "start_time": "2024-01-15T10:00:00Z",
  "end_time": "2024-01-15T10:05:30Z",
  "total_duration": 330,
  "final_score": 85.7,
  "goals_met": true,
  "metrics": {
    "evaluation_count": 10,
    "advisor_recommendations": 25,
    "changes_applied": 18
  }
}
```

### Statistics and Performance Metrics

Reports include detailed statistics:

- Execution duration by phase
- Tool execution times
- Evaluation score progression
- Number of recommendations generated
- Changes applied per iteration
- Resource usage metrics
- Success/failure rates

## Troubleshooting

### Monitor Issues

**Monitor not receiving messages:**

1. **Verify ailoop server is running:**
   ```bash
   # Check if ailoop is listening on the correct ports
   lsof -i :8080 -i :8081
   ```

2. **Check connection in monitor logs:**
   ```bash
   RUST_LOG=newton=debug,info newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080
   ```
   Look for "Subscription message sent successfully" and "Parsed message" logs.

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

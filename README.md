# Newton Loop - Anytime Optimization Framework

**Newton Loop** is a generic anytime optimization framework that orchestrates evaluation-advice-execution cycles to solve domain-agnostic problems through iterative improvement.

## What is Newton Loop?

Newton Loop is an iterative optimization framework for any agentic AI goal that can be well-defined in terms of goals and feedback (semantic gradients). It orchestrates a three-phase optimization cycle:

- **Evaluator**: Assesses the current state/solution and provides quality metrics
- **Advisor**: Generates improvement recommendations based on evaluation
- **Executor**: Implements the recommended changes to improve the solution

This evaluation-advice-execution loop continues until goals are met or iteration limits are reached.

Instead of just trying the same thing over and over and hoping it gets better, this kind of loop pauses to check how things are going, think about what could improve, and then make targeted changes. Each round learns from the last, so progress is more guided than random. It also keeps track of what worked best so far, which is helpful when goals involve trade-offs or gradual improvements rather than a simple yes/no result. Overall, it feels less like “try again” and more like “let’s see what happened and do a bit better next time,” which makes it a good fit for a wide range of problems.

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
Newton Loop optimization framework in Rust

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

Batch exposes the following environment variables for pre-run hooks, tool runs, and post hooks: `NEWTON_STATE_DIR`, `NEWTON_WS_ROOT`, `NEWTON_CODER_CMD`, `NEWTON_CONTROL_FILE`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`, `NEWTON_PRE_RUN` (1/0), `CODING_AGENT`, and `CODING_AGENT_MODEL`. Pre-run scripts also see `NEWTON_PROJECT_ROOT`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`, `NEWTON_GOAL_FILE`, and `NEWTON_RESUME`. Post hooks additionally receive `NEWTON_RESULT`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`, `NEWTON_STATE_DIR`, and `NEWTON_CONTROL_FILE` so they can reference the same artifacts without re-reading the plan.

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

Monitor ailoop channels in real-time using a TUI that highlights blocking questions and authorizations, keeps a queue of pending prompts, lets you answer/approve/deny directly in the terminal, and provides filtering (`/`), layout toggle (`V`), queue tab (`Q`), and help (`?`).

```bash
newton monitor
```

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

Newton Loop allows you to specify custom commands for each optimization phase. After `newton init .` the workspace already contains `.newton/scripts/{evaluator,advisor,executor}.sh`, so you can point overrides directly at the installed helpers:

```bash
newton run . \
  --evaluator ".newton/scripts/evaluator.sh" \
  --advisor ".newton/scripts/advisor.sh" \
  --executor ".newton/scripts/executor.sh"
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

## Configuration

### Workspace Structure

Newton Loop expects the following workspace structure:

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
│   └── prompts/            # Prompt templates
└── final_solution.json     # Final optimization result
```

### Batch Processing with Plans

Newton Loop uses a plan-based queue system to process work in batches. Plans live under `.newton/plan/{project_id}/` and travel through well-defined directories while each task maintains its own execution state:

1. **Create plan files** in `.newton/plan/{project_id}/todo/`. Each plan is a Markdown file describing the work, and optional YAML frontmatter can include metadata such as `branch: feature-name` or `coding_agent`.
2. **Plan lifecycle** moves files between directories:
   - `todo/` – awaiting execution
   - `completed/` – successfully finished
   - `failed/` – tasks that stopped with an error or non-success signal
   - `draft/` – work-in-progress plans not yet ready for batching
3. **Task execution state** is stored under `.newton/tasks/{task_id}/`, which contains:
   - `input/spec.md` – the original specification
   - `state/` – iteration/state artifacts such as prompts, promises, and control files
   - `output/` – results produced by tools or the executor

`newton batch` copies the plan into `project_root/.newton/tasks/{task_id}/input/spec.md` and reuses the same `task_id` when the plan is re-queued, making it easy to resume or re-run tasks without losing history.

Example plan file with frontmatter:
```markdown
---
branch: add-user-authentication
---

# Add User Authentication

Implement OAuth 2.0 authentication for the user login system, including refresh token rotation and role-based access controls.
```

### Git Integration

Newton Loop includes built-in git integration for batch processing to keep each task isolated:

- **Automatic branch creation**: Each plan can declare a `branch:` field in its frontmatter and Newton will create or check out that branch before running `newton run`.
- **Branch detection**: When a plan omits `branch`, Newton infers a feature branch name (`feature/{task_id}` with underscores replaced by dashes). Hooks and tools always see the resolved branch in `NEWTON_BRANCH_NAME`.
- **Commit support**: Executors and hooks can commit changes while on the task branch to capture iterative progress.
- **Pull request support**: After a successful task, you can rely on `newton batch` or post-success hooks to create PRs against the base branch tracked via `NEWTON_BASE_BRANCH`.

Branch names are derived from:
1. Plan file frontmatter (`branch:` field)
2. Current git branch (if already on a feature branch when the task started)
3. Environment variables (`NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`) passed through batch configs or hooks

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

### Environment Variables Available to Tools

Tools can access Newton Loop's environment variables:

| Variable | Purpose | Example |
|----------|---------|---------|
| `NEWTON_WORKSPACE_PATH` | Workspace root directory | `/path/to/workspace` |
| `NEWTON_ITERATION` | Current iteration number | `5` |
| `NEWTON_SCORE_FILE` | Evaluator output file | `/path/to/workspace/.newton/score.txt` |
| `NEWTON_STATE_DIR` | State directory | `/path/to/workspace/.newton/state` |
| `NEWTON_ARTIFACTS_DIR` | Artifacts directory | `/path/to/workspace/.newton/artifacts` |

#### Batch Processing Variables

- `NEWTON_PROJECT_ROOT`: Project root directory where tools operate
- `NEWTON_PROJECT_ID`: Identifier for the current batch project
- `NEWTON_TASK_ID`: Identifier for the current task within the plan
- `NEWTON_RESUME`: Flag (`true`/`false`) indicating whether to resume previous run
- `NEWTON_RESULT`: Result status of the previous iteration (`success`/`failure`)
- `NEWTON_CONTROL_FILE`: Path to the control file evaluated by the orchestrator
- `NEWTON_STATE_DIR`: Path to the per-task state directory
- `NEWTON_WS_ROOT`: Workspace root path discovered by the CLI
- `NEWTON_CODER_CMD`: Command used for the optional coder tool
- `NEWTON_BRANCH_NAME`: Task branch name derived from plan/frontmatter/env
- `NEWTON_BASE_BRANCH`: Git base branch against which PRs apply
- `NEWTON_PRE_RUN`: Flag (`1`/`0`) indicating pre-run hook execution
- `CODING_AGENT`: AI agent identifier (e.g., `claude`, `openai`)
- `CODING_AGENT_MODEL`: AI model identifier (e.g., `claude-sonnet-4-5`)

### Batch Configuration File (.newton/configs/*.conf)

Newton Loop uses simple key=value files under `.newton/configs/` to drive batch processing. Each project can have its own `<project_id>.conf`, and `newton init` seeds `default.conf` in the workspace root.

```conf
# Required settings
project_root=/path/to/project
coding_agent=claude
coding_model=claude-sonnet-4-5

# Optional settings
evaluator_cmd=custom_evaluator.sh
advisor_cmd=custom_advisor.sh
executor_cmd=custom_executor.sh
coder_cmd=custom_coder.sh

# Hook scripts
pre_run_script=/path/to/pre_run.sh
post_success_script=/path/to/post_success.sh
post_fail_script=/path/to/post_fail.sh

# Execution control
resume=true
max_iterations=10
max_time=3600
verbose=true
control_file=.newton/control.json
```

**Configuration Keys:**
- `project_root`: Root directory of your project where plans are executed
- `coding_agent`: AI agent to use for planning/execution (e.g., `"claude"`, `"openai"`)
- `coding_model`: Specific model identifier, such as `"claude-sonnet-4-5"`
- `evaluator_cmd`, `advisor_cmd`, `executor_cmd`, `coder_cmd`: Override commands for each tool (default to `.newton/scripts/{tool}.sh`)
- `pre_run_script`: Script invoked before each task (run from `project_root`)
- `post_success_script`: Script invoked after a successful task run
- `post_fail_script`: Script invoked after a failed task run
- `resume`: (`true`/`1`) reuse existing state directories; otherwise they are reset
- `max_iterations`: Maximum optimization iterations per task
- `max_time`: Maximum execution time in seconds
- `verbose`: Enable verbose logging for longer tool output
- `control_file`: Path (relative to `NEWTON_STATE_DIR` when not absolute) to the JSON file used for control signals

### Control Files for Success Signaling

Advanced batch workflows can write success/failure metadata to a control file that the orchestrator reads after each iteration. Configure the path using the `control_file` key in your batch config.

```json
{
  "success": true,
  "message": "All tests passed",
  "metadata": {
    "score": 95,
    "test_results": "..."
  }
}
```

The running tools or hooks write this JSON to `NEWTON_CONTROL_FILE`. Newton Loop considers the task successful only when `success` is `true`; all other states (missing file, `false`, parse errors, limits) mark the run as failed and move the plan into `failed/`.

### Hook Scripts

Use hook scripts to run auxiliary logic before or after tasks. Configure them in your batch config:

```conf
pre_run_script=/path/to/setup.sh
post_success_script=/path/to/cleanup_success.sh
post_fail_script=/path/to/cleanup_fail.sh
```

- **Pre-run hooks** execute before `newton run` starts (e.g., prepare environment, fetch dependencies). They receive batch variables such as `NEWTON_PROJECT_ROOT`, `NEWTON_PROJECT_ID`, `NEWTON_TASK_ID`, `NEWTON_GOAL_FILE`, and `NEWTON_RESUME`.
- **Post-success hooks** run only after a successful task completion. They inherit the batch environment plus `NEWTON_RESULT=success`, `NEWTON_BRANCH_NAME`, `NEWTON_BASE_BRANCH`, `NEWTON_STATE_DIR`, and `NEWTON_CONTROL_FILE` for downstream automation (deploy, notify, etc.).
- **Post-fail hooks** execute after a run ends in failure. Their exit code is ignored but they can inspect `NEWTON_RESULT=failure` to trigger rollback, alerts, or re-queue logic.

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

Newton Loop generates several artifacts during execution:

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

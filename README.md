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

## Quick Start

### Initialize a Newton Workspace

Instead of hand-crafting the `.newton/` layout, run `newton init` inside your project root. The command uses **aikit-sdk** to install the official Newton template, writes the default config, and places all helper scripts under `.newton/scripts/`. Once initialization completes, you can launch `newton run` from that directory without passing an explicit path.

### 1. Create a Workspace

```bash
mkdir my-optimization
cd my-optimization

# Define your optimization goal
cat > GOAL.md << 'EOF'
Improve code quality by reducing cyclomatic complexity in Python files
while maintaining functionality and test coverage.
EOF

# Create tools directory
mkdir -p tools
```

### 2. Configure Your Tools

Newton Loop uses external CLI tools for each phase. Create simple shell scripts:

```bash
# tools/evaluator.sh
cat > tools/evaluator.sh << 'EOF'
#!/bin/bash
# Your evaluation logic here
# Output a score to stdout or write to $NEWTON_SCORE_FILE
echo "42" > "$NEWTON_SCORE_FILE"
EOF
chmod +x tools/evaluator.sh
```

### 3. Run Optimization

```bash
newton run
```

Newton will:
1. Read GOAL.md
2. Execute your evaluator tool
3. Generate recommendations via advisor
4. Apply changes via executor
5. Repeat until goals are met

### 4. Check Results

```bash
# Check execution status
newton status <execution-id>

# View execution report
newton report <execution-id>

# Check for errors
newton error <execution-id>
```

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

## Commands Reference

### `run <workspace-path>`

Start optimization loop for a workspace.

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
newton run . --evaluator ./tools/my_evaluator.sh
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

## Advanced Usage

### Custom Tool Configuration

Newton Loop allows you to specify custom commands for each optimization phase:

```bash
newton run . \
  --evaluator "python tools/evaluator.py" \
  --advisor "python tools/advisor.py" \
  --executor "python tools/executor.py"
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
├── GOAL.md                 # Optimization objectives
├── tools/                  # Directory for tool scripts
│   ├── evaluator.sh        # Evaluation script
│   ├── advisor.sh          # Advisory script
│   └── executor.sh         # Execution script
├── .newton/                # Execution state (auto-generated)
└── artifacts/              # Generated artifacts (auto-generated)
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
- `NEWTON_SOLVER_INPUT_FILE`: Path to solver input file

### Environment Variables Available to Tools

Tools can access Newton Loop's environment variables:

| Variable | Purpose | Example |
|----------|---------|---------|
| `NEWTON_WORKSPACE_PATH` | Workspace root directory | `/path/to/workspace` |
| `NEWTON_ITERATION` | Current iteration number | `5` |
| `NEWTON_SCORE_FILE` | Evaluator output file | `/path/to/workspace/.newton/score.txt` |
| `NEWTON_STATE_DIR` | State directory | `/path/to/workspace/.newton/state` |
| `NEWTON_ARTIFACTS_DIR` | Artifacts directory | `/path/to/workspace/.newton/artifacts` |

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

## License

See LICENSE file for details.

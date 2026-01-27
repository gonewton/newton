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

```bash
brew install gonewton/tap/newton
```

### Windows (Scoop)

```powershell
scoop bucket add gonewton
scoop install newton
```

## Quick Start

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
newton run .
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
# newton 0.3.7

newton --help
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

**Examples:**
```bash
# Run with default settings
newton run .

# Run with custom timeouts
newton run . --max-iterations 100 --timeout 3600

# Use custom tools
newton run . --evaluator ./tools/my_evaluator.sh
```

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

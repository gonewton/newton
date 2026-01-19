# Quickstart: Rust Newton Loop Implementation

**Date**: 2026-01-19
**Feature**: 002-rust-newton-code

## Overview

The Rust Newton Loop implementation provides 100% API and behavioral compatibility with the Python version while leveraging Rust's performance and reliability benefits.

## Prerequisites

- **Rust nightly toolchain** (required for latest dependencies)
- **Python Newton Loop** (for comparison and integration testing)
- **Linux environment** (primary target platform)

## Installation

### From Source

```bash
# Clone the repository
git clone <repository-url>
cd newton-code

# Build with nightly toolchain
cargo build --release

# Verify installation
./target/release/newton-code --help
```

### Development Setup

```bash
# Install development dependencies
cargo install cargo-nextest
cargo install cargo-insta

# Run tests to verify setup
cargo nextest run

# Format and lint code
cargo fmt --all
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

## Basic Usage

### Run Optimization

```bash
# Execute optimization with explicit CLI tools (strict mode)
newton-code run /path/to/workspace \
  --evaluator-cmd './tools/evaluator.sh' \
  --advisor-cmd './tools/advisor.sh' \
  --executor-cmd './tools/executor.sh' \
  --max-iterations 10
```

### Check Status

```bash
# Monitor running optimization
newton-code status <execution-id> --workspace /path/to/workspace
```

### Generate Report

```bash
# Get optimization results
newton-code report <execution-id> --workspace /path/to/workspace

# JSON format for scripting
newton-code report <execution-id> --workspace /path/to/workspace --format json
```

### Single Step Execution

```bash
# Execute one iteration cycle
newton-code step /path/to/workspace --execution-id <optional-id>
```

## Workspace Structure

```
workspace/
├── problem/
│   ├── GOAL.md              # Optimization objectives
│   ├── CONSTRAINTS.md       # Problem constraints
│   └── data.json            # Problem-specific data
├── tools/
│   ├── evaluator.sh/.py     # Solution quality assessment
│   ├── advisor.sh/.py       # Improvement recommendations
│   └── executor.sh/.py      # Solution modification
├── solution.json            # Current solution state
└── artifacts/               # Generated during execution
    └── iter-{n}/
        ├── evaluator/       # Evaluation artifacts
        ├── advisor/         # Recommendation artifacts
        └── executor/        # Execution artifacts
```

## Tool Development

### Environment Variables

All tools receive these environment variables:

```bash
# Core variables (all tools)
NEWTON_WORKSPACE_PATH="/path/to/workspace"
NEWTON_EXECUTION_ID="abc123-def456"
NEWTON_ITERATION_NUMBER="1"

# Iteration-specific variables
NEWTON_ITERATION_DIR="/path/to/workspace/artifacts/iter-1"
NEWTON_EVALUATOR_DIR="/path/to/workspace/artifacts/iter-1/evaluator"
NEWTON_ADVISOR_DIR="/path/to/workspace/artifacts/iter-1/advisor"
NEWTON_EXECUTOR_DIR="/path/to/workspace/artifacts/iter-1/executor"
NEWTON_SCORE_FILE="/path/to/workspace/artifacts/score.txt"
```

### Tool Requirements

- **Exit codes**: 0 for success, non-zero for failure
- **Input**: Read exclusively from environment variables and workspace files
- **Output**: Write to designated artifact directories
- **Timeout**: Handle 30-second default timeout gracefully

### Evaluator Tool Example

```bash
#!/bin/bash
# Read current solution
solution=$(cat "$NEWTON_WORKSPACE_PATH/solution.json")

# Evaluate quality (0-1 scale)
score=$(compute_score "$solution")

# Write score to required location
echo "$score" > "$NEWTON_SCORE_FILE"

# Optional: write detailed evaluation to artifacts
echo "Evaluation details..." > "$NEWTON_EVALUATOR_DIR/details.md"
```

### Advisor Tool Example

```bash
#!/bin/bash
# Read evaluation results
score=$(cat "$NEWTON_SCORE_FILE")

# Generate recommendations
recommendations=$(generate_advice "$score")

# Write recommendations to artifacts
cat > "$NEWTON_ADVISOR_DIR/recommendations.md" << EOF
# Improvement Recommendations

$recommendations
EOF
```

### Executor Tool Example

```bash
#!/bin/bash
# Read advisor recommendations
recommendations=$(cat "$NEWTON_ADVISOR_DIR/recommendations.md")

# Apply changes to solution
updated_solution=$(apply_changes "$recommendations")

# Write updated solution
echo "$updated_solution" > "$NEWTON_WORKSPACE_PATH/solution.json"

# Log execution results
cat > "$NEWTON_EXECUTOR_DIR/log.md" << EOF
# Execution Log

Changes applied successfully at $(date)
EOF
```

## Compatibility Verification

### Against Python Version

```bash
# Run identical workloads with both versions
python -m newtonloop.cli run /workspace --evaluator-cmd ./eval --advisor-cmd ./advise --executor-cmd ./execute
newton-code run /workspace --evaluator-cmd ./eval --advisor-cmd ./advise --executor-cmd ./execute

# Compare outputs
diff python_output.txt rust_output.txt
```

### Test Suites

```bash
# Run comprehensive test suite
cargo nextest run

# Run CLI integration tests
cargo nextest run --package cli_tests

# Run compatibility tests
cargo nextest run compatibility
```

## Configuration

### Environment Variables

```bash
# Logging configuration
RUST_LOG=newton_code=debug cargo run --bin newton-code -- <args>

# Disable progress indicators
NEWTON_NO_PROGRESS=1 newton-code run /workspace <options>
```

### Timeouts and Limits

```bash
# Global tool timeout
newton-code run /workspace --tool-timeout-seconds 60 <other-options>

# Tool-specific timeouts
newton-code run /workspace \
  --evaluator-timeout 30 \
  --advisor-timeout 45 \
  --executor-timeout 60 \
  <other-options>
```

## Troubleshooting

### Common Issues

**Tool execution fails with timeout**
```bash
# Increase timeout
newton-code run /workspace --tool-timeout-seconds 120 <options>
```

**Workspace validation errors**
```bash
# Check workspace structure
ls -la /path/to/workspace
cat /path/to/workspace/problem/GOAL.md
```

**Environment variable issues**
```bash
# Debug environment variables in tool
env | grep NEWTON_
```

### Logging and Debugging

```bash
# Enable detailed logging
RUST_LOG=trace newton-code run /workspace <options>

# Check execution artifacts
ls -la /workspace/artifacts/
cat /workspace/artifacts/iter-1/evaluator/details.md
```

## Performance Comparison

### Benchmarks

```bash
# Time comparison (run multiple times for average)
time python -m newtonloop.cli run /workspace <options>
time newton-code run /workspace <options>
```

### Memory Usage

```bash
# Monitor memory usage
/usr/bin/time -v newton-code run /workspace <options>
```

## Development Workflow

### Running Tests

```bash
# All tests
cargo nextest run

# Specific test
cargo nextest run test_name

# With coverage
cargo llvm-cov nextest
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Lint code
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Check documentation
cargo doc --open
```

### Release Build

```bash
# Optimized release build
cargo build --release

# Run release tests
cargo nextest run --release
```

## Integration with Newton Loop

The Rust implementation is designed to be a drop-in replacement for the Python version:

- **API Compatibility**: Identical CLI interface and arguments
- **Behavioral Compatibility**: Same execution flow and error handling
- **Artifact Compatibility**: Same file formats and workspace structure
- **Environment Compatibility**: Same environment variable interface

Use the Python version for development and testing, then deploy the Rust version for production performance gains.
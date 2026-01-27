# Data Model: Rust Newton Loop Implementation

**Date**: 2026-01-27 (Updated)
**Feature**: 002-rust-newton-code

## Overview

The Rust Newton Loop implementation maintains identical data entities and validation rules to the Python version for 100% compatibility. All entities implement `Serialize`/`Deserialize` from serde and follow Rust ownership patterns with proper error handling.

## Core Entities

### OptimizationExecution

Represents a complete optimization run with execution tracking and final solution reference.

**Fields**:
- `execution_id: String` - Unique identifier (UUID format)
- `workspace_id: String` - Associated workspace identifier
- `problem_id: String` - Problem definition identifier
- `algorithm_parameters: HashMap<String, Value>` - Algorithm configuration
- `resource_limits: HashMap<String, Value>` - Time/iteration constraints
- `started_at: Option<DateTime<Utc>>` - Execution start timestamp
- `completed_at: Option<DateTime<Utc>>` - Execution completion timestamp
- `status: ExecutionStatus` - Current execution state
- `final_solution_id: Option<String>` - Best solution identifier
- `termination_reason: Option<String>` - Why execution stopped

**Validation Rules**:
- `execution_id` must be non-empty UUID
- `workspace_id` must be non-empty
- `problem_id` must be non-empty
- `started_at` must be before `completed_at` if both present
- `status` must be one of: pending, running, completed, failed, terminated
- `termination_reason` only set when status is terminated
- `final_solution_id` only set when status is completed
- `resource_limits` must contain max_iterations or max_time_seconds

**State Transitions**:
- pending → running (when execution starts)
- running → completed (successful finish)
- running → failed (execution error)
- running → terminated (resource limit exceeded)

### Iteration

Represents a single evaluation-advice-execution cycle within an optimization run.

**Fields**:
- `iteration_id: String` - Unique iteration identifier
- `execution_id: String` - Parent execution identifier
- `iteration_number: u32` - Sequential iteration count (1-based)
- `started_at: Option<DateTime<Utc>>` - Iteration start timestamp
- `completed_at: Option<DateTime<Utc>>` - Iteration completion timestamp
- `evaluation_result_id: Option<String>` - Evaluation result reference
- `advice_generated: HashMap<String, Value>` - Generated recommendations
- `changes_applied: Vec<HashMap<String, Value>>` - Applied modifications
- `status: IterationStatus` - Current iteration state

**Validation Rules**:
- `iteration_id` must be non-empty UUID
- `execution_id` must be non-empty
- `iteration_number` must be positive (>= 1)
- `started_at` must be before `completed_at` if both present
- `status` must be one of: running, completed, failed
- `evaluation_result_id` must exist when iteration completed successfully

### Workspace

Represents a problem-solving workspace with configuration and state.

**Fields**:
- `workspace_id: String` - Unique workspace identifier (default: UUID)
- `path: PathBuf` - Filesystem path to workspace root
- `created_at: DateTime<Utc>` - Creation timestamp
- `template_id: String` - Associated template identifier
- `status: WorkspaceStatus` - Current workspace state
- `configuration: HashMap<String, Value>` - Workspace configuration

**Validation Rules**:
- `workspace_id` must be non-empty
- `path` must exist and be accessible
- `status` must be one of: initializing, ready, optimizing, completed, error
- `template_id` must be non-empty if specified

### ErrorRecord

Captures error information with categorization and recovery suggestions.

**Fields**:
- `error_id: String` - Unique error identifier
- `error_category: ErrorCategory` - Error classification
- `error_severity: ErrorSeverity` - Error impact level
- `error_message: String` - Human-readable error description
- `error_code: String` - Machine-readable error code
- `error_context: HashMap<String, Value>` - Execution context
- `error_stack_trace: String` - Full stack trace
- `component_id: String` - Component where error occurred
- `error_timestamp: DateTime<Utc>` - When error occurred

**Validation Rules**:
- `error_id` must be non-empty UUID
- `error_category` must be valid ErrorCategory enum
- `error_severity` must be valid ErrorSeverity enum
- `error_message` must be non-empty
- `component_id` must be non-empty

### ToolResult

Represents the result of executing a CLI tool with timing and output capture.

**Fields**:
- `success: bool` - Whether tool execution succeeded
- `exit_code: i32` - Process exit code
- `execution_time: f64` - Time taken in seconds
- `stdout: String` - Standard output capture
- `stderr: String` - Standard error capture

**Validation Rules**:
- `exit_code` should be 0 for success=true, non-zero for success=false
- `execution_time` must be positive

## Enums and Types

### ExecutionStatus
```rust
enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Terminated,
}
```

### IterationStatus
```rust
enum IterationStatus {
    Running,
    Completed,
    Failed,
}
```

### WorkspaceStatus
```rust
enum WorkspaceStatus {
    Initializing,
    Ready,
    Optimizing,
    Completed,
    Error,
}
```

### ErrorCategory
```rust
enum ErrorCategory {
    Validation,
    Execution,
    Configuration,
    ToolFailure,
    Timeout,
    System,
}
```

### ErrorSeverity
```rust
enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}
```

## Relationships

- `OptimizationExecution` 1:N `Iteration` (execution contains multiple iterations)
- `OptimizationExecution` 1:1 `Workspace` (execution runs in one workspace)
- `Iteration` can reference `ErrorRecord` (if iteration failed)
- `ToolResult` associated with tool execution phases (evaluation, advice, execution)

## Storage and Persistence

- All entities serialized to JSON for workspace artifacts
- Execution state persisted to workspace directory
- Error records maintained in memory with optional persistence
- No database required - file-based persistence only

## Compatibility Notes

- All field names and types match Python dataclasses exactly
- Validation rules identical to Python implementation
- Serialization formats compatible with Newton Loop expectations
- Error categorization matches Python error handler
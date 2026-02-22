use serde::{Deserialize, Serialize};

/// Execution status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
    MaxIterationsReached,
    /// Legacy marker retained for backward compatibility with older persisted executions.
    Terminated,
}

/// Iteration status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IterationStatus {
    Running,
    Completed,
    Failed,
}

/// Error category enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    ValidationError,
    ToolExecutionError,
    TimeoutError,
    ResourceError,
    WorkspaceError,
    IterationError,
    SerializationError,
    IoError,
    ArtifactError,
    InternalError,
    Unknown,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Error severity enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Debug,
}

/// Iteration phase enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IterationPhase {
    #[default]
    Evaluator,
    Advisor,
    Executor,
    Complete,
}

/// Tool type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolType {
    Evaluator,
    Advisor,
    Executor,
}

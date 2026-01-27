use serde::{Deserialize, Serialize};

/// Execution status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Terminated,
}

/// Iteration status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IterationStatus {
    Running,
    Completed,
    Failed,
}

/// Workspace status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceStatus {
    Initializing,
    Ready,
    Optimizing,
    Completed,
    Error,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IterationPhase {
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

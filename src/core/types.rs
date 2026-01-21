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

use std::path::Path;

/// Workspace validation error
#[derive(Debug, thiserror::Error)]
#[error("Workspace validation error")]
pub enum WorkspaceValidationError {
    #[error("Path not found: {path}")]
    PathNotFound { path: String },
    #[error("Path is not a directory: {path}")]
    PathNotDirectory { path: String },
    #[error("Configuration file missing: {file}")]
    ConfigFileMissing { file: String },
    #[error("Invalid structure: {message}")]
    InvalidStructure { message: String },
}

/// Test reporter trait
pub trait TestReporterTrait {
    fn report(&self, message: &str);
}

/// Test validator trait
pub trait TestValidatorTrait {
    fn validate(&self) -> std::result::Result<(), WorkspaceValidationError>;
}

/// Workspace validator trait
pub trait WorkspaceValidatorTrait {
    fn validate_path(&self, path: &Path) -> std::result::Result<(), WorkspaceValidationError>;
    fn validate_structure(&self, path: &Path) -> std::result::Result<(), WorkspaceValidationError>;
    fn validate_configuration(
        &self,
        path: &Path,
    ) -> std::result::Result<(), WorkspaceValidationError>;
    fn is_locked(&self, path: &Path) -> bool;
}

/// Test validator trait
pub trait TestValidator {
    fn validate(&self) -> std::result::Result<(), WorkspaceValidationError>;
}

/// Workspace validator trait
pub trait WorkspaceValidator {
    fn validate(&self, path: &Path) -> std::result::Result<(), WorkspaceValidationError>;
}

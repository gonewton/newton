pub mod entities;
pub mod error;
pub mod history_recorder;
pub mod logger;
pub mod orchestrator;
pub mod results_processor;
pub mod tool_executor;
pub mod types;
pub mod workspace;

pub use entities::{
    ArtifactError, ArtifactMetadata, ErrorCategory, ExecutionStatus, IterationPhase,
    OptimizationExecution, ToolResult, ToolType, Workspace, WorkspaceStatus,
};
pub use error::{AppError, DefaultErrorReporter, ErrorReporter, ErrorSeverity};
pub use history_recorder::ExecutionHistoryRecorder;
pub use orchestrator::OptimizationOrchestrator;
pub use results_processor::{OutputFormat, ResultsProcessor};
pub use types::*;
pub use workspace::WorkspaceManager;

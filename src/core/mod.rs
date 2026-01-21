pub mod entities;
pub mod error;
pub mod history_recorder;
pub mod logger;
pub mod orchestrator;
pub mod performance;
pub mod results_processor;
pub mod tool_executor;
pub mod types;
pub mod workspace;

pub use crate::tools::ToolResult;
pub use entities::{
    ArtifactMetadata, ErrorCategory, ExecutionStatus, IterationPhase, OptimizationExecution,
    ToolType, Workspace, WorkspaceStatus,
};
pub use error::{AppError, DefaultErrorReporter, ErrorReporter};
pub use history_recorder::ExecutionHistoryRecorder;
pub use orchestrator::OptimizationOrchestrator;
pub use performance::PerformanceProfiler;
pub use results_processor::{OutputFormat, ResultsProcessor};
pub use types::*;
pub use workspace::WorkspaceManager;

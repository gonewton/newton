pub mod config;
pub mod entities;
pub mod error;
pub mod git;
pub mod history_recorder;
pub mod logger;
pub mod orchestrator;
pub mod performance;
pub mod results_processor;
pub mod success_policy;
pub mod tool_executor;
pub mod types;
pub mod workspace;

pub use crate::tools::ToolResult;
pub use config::{ConfigLoader, ConfigValidator, NewtonConfig};
pub use entities::{
    ArtifactMetadata, ErrorCategory, ExecutionStatus, IterationPhase, OptimizationExecution,
    ToolType,
};
pub use error::{AppError, DefaultErrorReporter, ErrorReporter};
pub use git::{BranchManager, CommitManager, GitManager, PullRequestManager};
pub use history_recorder::ExecutionHistoryRecorder;
pub use orchestrator::OptimizationOrchestrator;
pub use performance::PerformanceProfiler;
pub use results_processor::{OutputFormat, ResultsProcessor};
pub use success_policy::SuccessPolicy;
pub use types::*;

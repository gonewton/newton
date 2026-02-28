//! Core Newton domain models, configuration, and orchestration utilities that drive workspace executions.
pub mod batch_config;
pub mod config;
pub mod context;
pub mod error;
pub mod git;
pub mod logger;
pub mod performance;
pub mod template;
pub mod types;
pub mod workflow_graph;
pub mod workspace;

pub use batch_config::{find_workspace_root, parse_conf, BatchProjectConfig};
pub use config::{ConfigLoader, ConfigValidator, NewtonConfig};
pub use context::ContextManager;
pub use error::{AppError, DefaultErrorReporter, ErrorReporter};
pub use git::{BranchManager, CommitManager, GitManager, PullRequestManager};
pub use performance::PerformanceProfiler;
pub use template::{TemplateInfo, TemplateManager, TemplateRenderer};
pub use types::*;

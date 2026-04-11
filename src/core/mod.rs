//! Core Newton domain models, configuration, and orchestration utilities that drive workspace executions.
pub mod batch_config;
pub mod config;
pub mod context_file;
pub mod error;
pub mod logger;
pub mod performance;
pub mod template;
pub mod types;
pub mod workflow_graph;
pub mod workspace;

/// Backward-compatible re-export: `core::context` still resolves to `core::context_file`.
pub mod context {
    pub use super::context_file::*;
}

pub use batch_config::{find_workspace_root, parse_conf, BatchProjectConfig};
pub use config::{ConfigLoader, ConfigValidator, NewtonConfig};
pub use context_file::ContextManager;
pub use error::{AppError, DefaultErrorReporter, ErrorReporter};
pub use performance::PerformanceProfiler;
pub use template::{TemplateInfo, TemplateManager, TemplateRenderer};
pub use types::*;

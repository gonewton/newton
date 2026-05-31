//! Core Newton domain models, configuration, and orchestration utilities that drive workspace executions.
pub mod batch_config;
pub mod config;
pub mod context_file;
pub mod error;
pub mod template;
pub mod types;
pub mod workspace;

pub use batch_config::{find_workspace_root, parse_conf, BatchProjectConfig};
pub use config::{validate_config, ConfigLoader, NewtonConfig};
pub use context_file::ContextManager;
pub use error::{AppError, DefaultErrorReporter, ErrorReporter};
pub use template::{TemplateInfo, TemplateManager, TemplateRenderer};
pub use types::*;

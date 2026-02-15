pub mod config;
pub mod orchestrator_notifier;
pub mod output_forwarder;
pub mod tool_client;
pub mod workflow_emitter;

use crate::cli::Command;
use crate::Result;
use std::path::Path;

pub use config::{AiloopConfig, AiloopContext};
pub use orchestrator_notifier::OrchestratorNotifier;
pub use output_forwarder::OutputForwarder;
pub use tool_client::ToolClient;
pub use workflow_emitter::WorkflowEmitter;

/// Initialize ailoop integration context for a given command and workspace.
/// Returns None if ailoop integration is disabled or not configured.
pub fn init_context(workspace_root: &Path, command: &Command) -> Result<Option<AiloopContext>> {
    config::init_context(workspace_root, command)
}

pub mod bounded_queue;
pub mod config;
pub mod orchestrator_notifier;
pub mod output_forwarder;
pub mod tool_client;
pub mod workflow_emitter;

use crate::ailoop_integration::config::AiloopConfig;
use crate::ailoop_integration::orchestrator_notifier::OrchestratorNotifier;
use crate::ailoop_integration::output_forwarder::OutputForwarder;
use crate::ailoop_integration::tool_client::ToolClient;
use crate::cli::Command;
use crate::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// Tracks whether the ailoop transport has reported a failure.
#[derive(Debug)]
pub struct TransportState {
    failure: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl TransportState {
    /// Create a fresh transport health tracker.
    pub fn new() -> Self {
        TransportState {
            failure: AtomicBool::new(false),
            last_error: Mutex::new(None),
        }
    }

    /// Record the first failure message and keep the flag set.
    pub fn mark_failure(&self, message: impl Into<String>) {
        let already_failed = self.failure.swap(true, Ordering::SeqCst);
        if !already_failed {
            let mut guard = self.last_error.lock().unwrap();
            *guard = Some(message.into());
        }
    }

    /// Return whether a failure has been recorded.
    pub fn has_failure(&self) -> bool {
        self.failure.load(Ordering::SeqCst)
    }

    /// Return the recorded failure message, if any.
    pub fn failure_message(&self) -> Option<String> {
        let guard = self.last_error.lock().unwrap();
        guard.clone()
    }
}

impl Default for TransportState {
    fn default() -> Self {
        Self::new()
    }
}

/// Context that wires together the ailoop helper components.
pub struct AiloopContext {
    config: Arc<AiloopConfig>,
    notifier: OrchestratorNotifier,
    forwarder: OutputForwarder,
    tool_client: ToolClient,
    state: Arc<TransportState>,
}

impl AiloopContext {
    fn new(config: AiloopConfig) -> Self {
        let shared = Arc::new(config);
        let client = Client::new();
        let state = Arc::new(TransportState::new());
        let notifier = OrchestratorNotifier::new(shared.clone(), client.clone(), state.clone());
        let forwarder = OutputForwarder::new(shared.clone(), client.clone(), state.clone());
        let tool_client = ToolClient::new(shared.clone(), client);
        AiloopContext {
            config: shared,
            notifier,
            forwarder,
            tool_client,
            state,
        }
    }

    /// Inject ailoop environment variables for subprocesses.
    pub fn inject_env(&self, env: &mut HashMap<String, String>) {
        env.insert("NEWTON_AILOOP_ENABLED".to_string(), "1".to_string());
        env.insert(
            "NEWTON_AILOOP_HTTP_URL".to_string(),
            self.config.http_url.to_string(),
        );
        env.insert(
            "NEWTON_AILOOP_WS_URL".to_string(),
            self.config.ws_url.to_string(),
        );
        env.insert(
            "NEWTON_AILOOP_CHANNEL".to_string(),
            self.config.channel.clone(),
        );
    }

    /// Access the orchestrator lifecycle notifier.
    pub fn notifier(&self) -> &OrchestratorNotifier {
        &self.notifier
    }

    /// Access the output forwarder used by tool subprocesses.
    pub fn output_forwarder(&self) -> &OutputForwarder {
        &self.forwarder
    }

    /// Access the helper API for tool scripts.
    pub fn tool_client(&self) -> &ToolClient {
        &self.tool_client
    }

    /// Return whether the integration was configured in fail-fast mode.
    pub fn fail_fast(&self) -> bool {
        self.config.fail_fast
    }

    /// Return a workspace identifier that can be included in events.
    pub fn workspace_identifier(&self) -> &str {
        &self.config.workspace_identifier
    }

    /// Return the captured command context.
    pub fn command_context(&self) -> crate::ailoop_integration::config::CommandContext {
        self.config.command_context.clone()
    }

    /// Indicate whether a transport failure has been recorded.
    pub fn has_transport_failure(&self) -> bool {
        self.state.has_failure()
    }

    /// Return the recorded transport failure message, if any.
    pub fn transport_failure_message(&self) -> Option<String> {
        self.state.failure_message()
    }

    /// Flush pending ailoop messages and stop background workers.
    pub async fn shutdown(self) -> crate::Result<()> {
        if let Err(err) = self.notifier.shutdown().await {
            tracing::warn!(error = ?err, "failed to shut down ailoop notifier");
        }
        if let Err(err) = self.forwarder.shutdown().await {
            tracing::warn!(error = ?err, "failed to shut down ailoop forwarder");
        }
        Ok(())
    }
}

/// Resolve and initialize the integration context for the provided command.
pub fn init_context(workspace_root: &Path, command: &Command) -> Result<Option<AiloopContext>> {
    match config::init_config(workspace_root, command) {
        Ok(Some(conf)) => Ok(Some(AiloopContext::new(conf))),
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    }
}

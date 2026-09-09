//! Execute optimization steps through the normal loader, registry and state root.

use anyhow::{anyhow, Context};
use newton_core::workflow::{executor::ExecutionSummary, schema::WorkflowTrigger};
use newton_types::optimization::{ExecutionAction, ExecutionAuthority};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct WorkflowExecutor {
    pub workspace: PathBuf,
    pub state_dir: PathBuf,
}

impl WorkflowExecutor {
    pub async fn execute(
        &self,
        path: &Path,
        triggers: Value,
        remaining_seconds: u64,
        authority: &ExecutionAuthority,
    ) -> anyhow::Result<ExecutionSummary> {
        if remaining_seconds == 0 {
            anyhow::bail!("optimization elapsed-time budget exhausted before dispatch");
        }
        let (mut document, _) = newton_core::workflow::loader::load_and_lint_workflow(path)
            .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;
        validate_authority(&document, authority)?;
        document.triggers = Some(WorkflowTrigger::manual(triggers.clone()));
        let settings = &document.workflow.settings;
        if let Some(limit) = settings.io_settings.max_input_bytes {
            if serde_json::to_vec(&triggers)?.len() > limit {
                anyhow::bail!("WFG-IO-001: optimization step input exceeds {limit} bytes");
            }
        }
        if let Some(schema) = &settings.io.input_schema {
            newton_core::workflow::io::validate_input_schema(schema, &triggers)
                .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;
        }
        let setup = super::super::shared_execution::build_execution_setup(
            self.state_dir.clone(),
            None,
            Some(remaining_seconds),
            None,
        )
        .await
        .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;
        let hil = newton_core::integrations::ailoop::init_context_for_command_name(
            &self.workspace,
            "optimize",
        )
        .map_err(|e| anyhow!("optimization HIL configuration: {e}"))?;
        let registry = super::super::build_operator_registry(
            self.workspace.clone(),
            &self.state_dir,
            settings,
            hil,
        )
        .await;
        // Enforce the deadline around the entire step, including agent calls and
        // retries. A global workflow deadline alone is checked between ticks.
        let summary = tokio::time::timeout(
            std::time::Duration::from_secs(remaining_seconds),
            newton_core::workflow::executor::execute_workflow(
                document,
                path.to_path_buf(),
                registry,
                self.workspace.clone(),
                setup.overrides,
            ),
        )
        .await
        .context("optimization elapsed-time budget exhausted during workflow")?
        .map_err(|e| anyhow!("optimization workflow failed: {e}"))?;
        if !summary.output_valid {
            anyhow::bail!("optimization workflow produced invalid output");
        }
        Ok(summary)
    }
}

pub(super) fn validate_authority(
    document: &newton_core::workflow::schema::WorkflowDocument,
    authority: &ExecutionAuthority,
) -> anyhow::Result<()> {
    for task in document.workflow.tasks() {
        if matches!(
            task.operator.as_str(),
            "NoOpOperator" | "SetContextOperator" | "BarrierOperator" | "AssertCompletedOperator"
        ) {
            continue;
        }
        // Existing executable operators run trusted host code without a sandbox.
        // Granting 'command' alone cannot enforce no-network/no-deploy on shell
        // or nested workflows. Refuse a partial authority instead of pretending.
        for action in [
            ExecutionAction::Agent,
            ExecutionAction::Command,
            ExecutionAction::Network,
            ExecutionAction::Commit,
            ExecutionAction::DraftPullRequest,
            ExecutionAction::Publish,
            ExecutionAction::Merge,
            ExecutionAction::Deploy,
        ] {
            newton_core::optimization::authorize_action(authority, action).map_err(|e|
                anyhow!("{} cannot enforce partial action authority with the unsandboxed workflow host: {e}", task.operator))?;
        }
    }
    Ok(())
}

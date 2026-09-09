//! Execute optimization steps through the normal loader, registry and state root.

use anyhow::anyhow;
use newton_core::workflow::{
    executor::ExecutionSummary,
    schema::{WorkflowDocument, WorkflowTrigger},
};
use newton_types::{
    optimization::{ExecutionAction, ExecutionAuthority, OptimizationRequirements},
    BackendStore,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct WorkflowExecutor {
    pub workspace: PathBuf,
    pub state_dir: PathBuf,
    pub store: Arc<dyn BackendStore>,
}

/// GradeOutput is one aggregate envelope, not a mergeable evaluator fragment.
/// Shared-workflow evaluator identities execute together once per sample.
pub(super) fn grade_reference(requirements: &OptimizationRequirements) -> anyhow::Result<&str> {
    let first = requirements
        .evaluators
        .values()
        .next()
        .ok_or_else(|| anyhow!("grading requires an active evaluator"))?;
    if requirements
        .evaluators
        .values()
        .any(|evaluator| evaluator.workflow != first.workflow)
    {
        anyhow::bail!("native aggregate GradeOutput requires all active evaluators to share one workflow; compose evaluators in that workflow instead of merging incompatible envelopes");
    }
    Ok(&first.workflow)
}

impl WorkflowExecutor {
    pub async fn execute(
        &self,
        path: &Path,
        mut document: WorkflowDocument,
        triggers: Value,
        remaining_seconds: u64,
        authority: &ExecutionAuthority,
    ) -> anyhow::Result<ExecutionSummary> {
        if remaining_seconds == 0 {
            return Err(super::stop::ResourceExhausted {
                uncertain: false,
                detail: ": elapsed time exhausted before dispatch",
            }
            .into());
        }
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
        let setup = super::super::shared_execution::build_execution_setup_with_backend(
            self.state_dir.clone(),
            None,
            Some(remaining_seconds),
            None,
            self.store.clone(),
        )
        .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;
        let hil = newton_core::integrations::ailoop::init_context_for_command_name(
            &self.workspace,
            "optimize",
        )
        .map_err(|e| anyhow!("optimization HIL configuration: {e}"))?;
        let registry = super::super::build_operator_registry_with_backend(
            self.workspace.clone(),
            settings,
            hil,
            Some(self.store.clone()),
        );
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
        .map_err(|_| super::stop::ResourceExhausted {
            uncertain: true,
            detail: ": workflow deadline expired; reconcile any external effects",
        })?
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
        if task.operator == "WorkflowOperator" {
            anyhow::bail!(
                "WorkflowOperator is unsupported in optimization roles because filesystem-loaded child workflows do not provide immutable execution provenance"
            );
        }
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

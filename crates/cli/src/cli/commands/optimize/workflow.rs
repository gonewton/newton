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
    collections::{HashMap, VecDeque},
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
        validate_resource_metering(&document)?;
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
            Some(0),
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

/// Reject Newton-controlled repetition that cannot be charged separately to the
/// optimization work/evaluation counters.
pub(super) fn validate_resource_metering(document: &WorkflowDocument) -> anyhow::Result<()> {
    for task in document.workflow.tasks() {
        if let Some(retry) = task.retry.as_ref().filter(|retry| retry.max_attempts > 1) {
            anyhow::bail!(
                "optimization task '{}' declares retry.max_attempts={}; this host cannot meter internal task retries against work/evaluation limits; omit retry or set max_attempts: 1 and use driver-managed retries",
                task.id,
                retry.max_attempts
            );
        }
        if task.operator == "AgentOperator"
            && task
                .params
                .get("loop")
                .is_some_and(|value| value != &Value::Bool(false))
        {
            anyhow::bail!(
                "optimization task '{}' enables or dynamically selects AgentOperator loop mode; this host cannot meter internal agent-loop iterations; omit loop or set loop: false and use driver-managed retries",
                task.id
            );
        }
        validate_operator_retries(task)?;
    }
    if let Some(task) = cyclic_task(document) {
        anyhow::bail!(
            "optimization workflow contains a transition cycle involving task '{task}'; this host cannot meter repeated graph executions against work/evaluation limits; use an acyclic role workflow and driver-managed retries"
        );
    }
    Ok(())
}

fn validate_operator_retries(
    task: &newton_core::workflow::schema::WorkflowTask,
) -> anyhow::Result<()> {
    let operation = task.params.get("operation");
    match task.operator.as_str() {
        "GitOperator" => match operation.and_then(Value::as_str) {
            Some("push") if task.params.get("retry_count").and_then(Value::as_u64) != Some(1) => {
                anyhow::bail!(
                    "optimization task '{}' uses GitOperator push without retry_count: 1; operator-internal retries are not separately metered",
                    task.id
                );
            }
            Some(_) => {}
            None => anyhow::bail!(
                "optimization task '{}' must use a literal GitOperator operation so internal retry behavior is known before dispatch",
                task.id
            ),
        },
        "GhOperator" => match operation.and_then(Value::as_str) {
            Some("pr_create" | "branch_push")
                if task.params.get("retry_count").and_then(Value::as_u64) != Some(1) =>
            {
                anyhow::bail!(
                    "optimization task '{}' uses a retrying GhOperator operation without retry_count: 1; operator-internal retries are not separately metered",
                    task.id
                );
            }
            Some("project_item_set_status") => anyhow::bail!(
                "optimization task '{}' uses GhOperator project_item_set_status, whose internal retry is not separately metered",
                task.id
            ),
            Some(_) => {}
            None => anyhow::bail!(
                "optimization task '{}' must use a literal GhOperator operation so internal retry behavior is known before dispatch",
                task.id
            ),
        },
        _ => {}
    }
    Ok(())
}

fn cyclic_task(document: &WorkflowDocument) -> Option<String> {
    let tasks = document.workflow.tasks().collect::<Vec<_>>();
    let mut indegree = tasks
        .iter()
        .map(|task| (task.id.clone(), 0_usize))
        .collect::<HashMap<_, _>>();
    let mut edges = HashMap::<String, Vec<String>>::new();
    for task in &tasks {
        for transition in &task.transitions {
            if let Some(value) = indegree.get_mut(&transition.to) {
                *value += 1;
                edges
                    .entry(task.id.clone())
                    .or_default()
                    .push(transition.to.clone());
            }
        }
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<VecDeque<_>>();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        for target in edges.get(&id).into_iter().flatten() {
            let count = indegree
                .get_mut(target)
                .expect("known transition target was inserted above");
            *count -= 1;
            if *count == 0 {
                ready.push_back(target.clone());
            }
        }
    }
    (visited != tasks.len()).then(|| {
        indegree
            .into_iter()
            .find(|(_, count)| *count > 0)
            .map(|(id, _)| id)
            .unwrap_or_else(|| "unknown".to_string())
    })
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

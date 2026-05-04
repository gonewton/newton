#![allow(clippy::result_large_err)]

pub mod agent;
pub mod assert_completed;
pub mod barrier;
pub mod command;
pub mod engine;
pub mod gh;
pub mod gh_authorization;
pub mod human_approval;
pub mod human_decision;
pub mod noop;
pub mod read_control_file;
pub mod set_context;
pub mod workflow;

use crate::workflow::child_run::ChildWorkflowRunner;
use crate::workflow::human::InterviewerProvider;
use crate::workflow::operator::OperatorRegistryBuilder;
use crate::workflow::operators::engine::AikitEngineManager;
use crate::workflow::state::GraphSettings;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub struct BuiltinOperatorDeps {
    /// Lazy provider that resolves to an `Interviewer` on first human prompt.
    /// Workflows with no human task never invoke the provider.
    pub interviewer: Option<InterviewerProvider>,
    pub command_runner: Option<Arc<dyn command::CommandRunner>>,
    /// GhRunner for GhOperator. Defaults to real gh CLI subprocess when None.
    pub gh_runner: Option<Arc<dyn gh::GhRunner>>,
    /// Child workflow runner for WorkflowOperator. Defaults to in-process execution when None.
    pub child_workflow_runner: Option<Arc<dyn ChildWorkflowRunner>>,
    /// Ailoop approver for GhOperator. Defaults to NoopApprover when None.
    pub gh_approver: Option<Arc<dyn gh_authorization::AiloopApprover>>,
}

/// Register built-in operators into the supplied builder.
pub fn register_builtins(
    builder: &mut OperatorRegistryBuilder,
    workspace: PathBuf,
    settings: GraphSettings,
) {
    register_builtins_with_deps(builder, workspace, settings, BuiltinOperatorDeps::default());
}

pub fn register_builtins_with_deps(
    builder: &mut OperatorRegistryBuilder,
    workspace: PathBuf,
    settings: GraphSettings,
    deps: BuiltinOperatorDeps,
) {
    let interviewer_provider: InterviewerProvider = deps.interviewer.unwrap_or_else(|| {
        // Default provider: every invocation returns HIL-AILOOP-001 because
        // no ailoop context was wired in. Workflows with no human task
        // never invoke this provider.
        Arc::new(|| {
            Err(crate::core::error::AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "human-in-the-loop operator requires an enabled ailoop context; \
                     configure ailoop (.newton/configs/monitor.conf and \
                     NEWTON_AILOOP_INTEGRATION=1). See \
                     docs/operators/human_decision.md#configuration",
            )
            .with_code("HIL-AILOOP-001"))
        })
    });
    let human_settings = settings.human.clone();
    let redact_keys = Arc::new(settings.redaction.redact_keys.clone());
    let command_operator = match deps.command_runner {
        Some(runner) => command::CommandOperator::with_runner(workspace.clone(), runner),
        None => command::CommandOperator::new(workspace.clone()),
    };
    let engine_manager = AikitEngineManager::new(workspace.clone())
        .expect("AikitEngineManager::new should not fail");
    let agent_operator = agent::AgentOperator::new(workspace, settings, engine_manager);
    let gh_operator = match (deps.gh_runner, deps.gh_approver) {
        (Some(runner), Some(approver)) => {
            gh::GhOperator::with_runner_and_approver(runner, approver)
        }
        (Some(runner), None) => gh::GhOperator::with_runner(runner),
        (None, Some(approver)) => {
            gh::GhOperator::with_runner_and_approver(Arc::new(gh::default_runner()), approver)
        }
        (None, None) => gh::GhOperator::new(),
    };
    let child_runner: Arc<dyn ChildWorkflowRunner> =
        deps.child_workflow_runner.unwrap_or_else(|| {
            Arc::new(crate::workflow::executor::InProcessChildWorkflowRunner::new())
        });
    builder
        .register(noop::NoOpOperator::new())
        .register(command_operator)
        .register(assert_completed::AssertCompletedOperator::new())
        .register(barrier::BarrierOperator::new())
        .register(set_context::SetContextOperator::new())
        .register(read_control_file::ReadControlFileOperator::new())
        .register(workflow::WorkflowOperator::new(child_runner))
        .register(agent_operator)
        .register(gh_operator)
        .register(human_approval::HumanApprovalOperator::new(
            interviewer_provider.clone(),
            human_settings.clone(),
            redact_keys.clone(),
        ))
        .register(human_decision::HumanDecisionOperator::new(
            interviewer_provider,
            human_settings,
            redact_keys,
        ));
}

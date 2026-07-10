pub mod agent;
pub mod assert_completed;
pub mod assessment;
pub mod barrier;
pub mod change_request_op;
pub mod command;
pub mod engine;
pub mod gh;
pub mod gh_authorization;
pub mod git;
pub mod grader_agent;
pub mod grader_command;
pub mod human_approval;
pub mod human_decision;
pub mod noop;
pub mod read_control_file;
pub mod reconcile;
pub mod set_context;
pub mod workflow;

use crate::workflow::child_run::ChildWorkflowRunner;
use crate::workflow::human::InterviewerProvider;
use crate::workflow::operator::OperatorRegistryBuilder;
use crate::workflow::operators::engine::AikitEngineManager;
use crate::workflow::state::GraphSettings;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared cap on inline-captured stdout/stderr bytes (`command` and `agent`
/// operators both apply this to the `stdout`/`stderr` fields they place in
/// task output). Exposed so callers that print captured output (e.g.
/// `workflow run --verbose`) can tell whether a stream was cut off at
/// capture time.
pub(crate) const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 1_048_576;

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
    /// GitRunner for GhOperator branch_push. Defaults to TokioGitRunner when None.
    pub git_runner: Option<Arc<dyn gh::GitRunner>>,
    /// BackendStore for grading operators (GraderCommandOperator, ReconcileOperator, etc.).
    pub backend_store: Option<Arc<dyn newton_types::BackendStore>>,
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
    let agent_operator = agent::AgentOperator::new(workspace.clone(), settings, engine_manager);
    let git_runner: Arc<dyn gh::GitRunner> = deps
        .git_runner
        .unwrap_or_else(|| Arc::new(gh::default_git_runner()));
    let gh_operator = match (deps.gh_runner, deps.gh_approver) {
        (Some(runner), Some(approver)) => gh::GhOperator::with_all(runner, git_runner, approver),
        (Some(runner), None) => {
            gh::GhOperator::with_all(runner, git_runner, Arc::new(gh_authorization::NoopApprover))
        }
        (None, Some(approver)) => {
            gh::GhOperator::with_all(Arc::new(gh::default_runner()), git_runner, approver)
        }
        (None, None) => gh::GhOperator::with_all(
            Arc::new(gh::default_runner()),
            git_runner,
            Arc::new(gh_authorization::NoopApprover),
        ),
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
        .register(git::GitOperator::new())
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

    // Descriptor/execution split (ADR-0014): the four optimization-loop
    // operators are always part of the described vocabulary — regardless of
    // whether a BackendStore is available in this context — so
    // `newton schema export`, the composed workflow schema, and DSL codegen
    // never lose them. Only the executable half below is store-gated.
    builder
        .register_descriptor(grader_command::GraderCommandOperator::descriptor())
        .register_descriptor(reconcile::ReconcileOperator::descriptor())
        .register_descriptor(change_request_op::ChangeRequestOperator::descriptor())
        .register_descriptor(grader_agent::GraderAgentOperator::descriptor());

    if let Some(store) = deps.backend_store {
        let grading_engine = AikitEngineManager::new(workspace.clone())
            .expect("AikitEngineManager::new should not fail");
        builder
            .register_executable_only(grader_command::GraderCommandOperator::new(
                workspace.clone(),
                store.clone(),
            ))
            .register_executable_only(reconcile::ReconcileOperator::new(
                workspace.clone(),
                store.clone(),
            ))
            .register_executable_only(change_request_op::ChangeRequestOperator::new(
                workspace.clone(),
                store.clone(),
            ))
            .register_executable_only(grader_agent::GraderAgentOperator::new(
                workspace,
                store,
                grading_engine,
            ));
    }
}

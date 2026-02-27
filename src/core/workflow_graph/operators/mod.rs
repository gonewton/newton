pub mod agent;
pub mod assert_completed;
pub mod command;
pub mod engine;
pub mod human_approval;
pub mod human_decision;
pub mod noop;
pub mod read_control_file;
pub mod set_context;

use crate::core::workflow_graph::human::{ConsoleInterviewer, Interviewer};
use crate::core::workflow_graph::operator::OperatorRegistryBuilder;
use crate::core::workflow_graph::operators::engine::EngineDriver;
use crate::core::workflow_graph::state::GraphSettings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub struct BuiltinOperatorDeps {
    pub interviewer: Option<Arc<dyn Interviewer>>,
    pub command_runner: Option<Arc<dyn command::CommandRunner>>,
    /// Engine driver registry for AgentOperator. Defaults to engine::default_registry() when None.
    pub engine_registry: Option<HashMap<String, Box<dyn EngineDriver>>>,
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
    let interviewer: Arc<dyn Interviewer> = deps
        .interviewer
        .unwrap_or_else(|| Arc::new(ConsoleInterviewer::new()));
    let human_settings = settings.human.clone();
    let redact_keys = Arc::new(settings.redaction.redact_keys.clone());
    let command_operator = match deps.command_runner {
        Some(runner) => command::CommandOperator::with_runner(workspace.clone(), runner),
        None => command::CommandOperator::new(workspace.clone()),
    };
    let engine_registry = deps
        .engine_registry
        .unwrap_or_else(engine::default_registry);
    let agent_operator = agent::AgentOperator::new(workspace, settings, engine_registry);
    builder
        .register(noop::NoOpOperator::new())
        .register(command_operator)
        .register(assert_completed::AssertCompletedOperator::new())
        .register(set_context::SetContextOperator::new())
        .register(read_control_file::ReadControlFileOperator::new())
        .register(agent_operator)
        .register(human_approval::HumanApprovalOperator::new(
            interviewer.clone(),
            human_settings.clone(),
            redact_keys.clone(),
        ))
        .register(human_decision::HumanDecisionOperator::new(
            interviewer,
            human_settings,
            redact_keys,
        ));
}

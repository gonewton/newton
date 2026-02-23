pub mod assert_completed;
pub mod command;
pub mod human_approval;
pub mod human_decision;
pub mod noop;
pub mod read_control_file;
pub mod set_context;

use crate::core::workflow_graph::human::{ConsoleInterviewer, Interviewer};
use crate::core::workflow_graph::operator::OperatorRegistryBuilder;
use crate::core::workflow_graph::state::GraphSettings;
use std::path::PathBuf;
use std::sync::Arc;

/// Register built-in operators into the supplied builder.
pub fn register_builtins(
    builder: &mut OperatorRegistryBuilder,
    workspace: PathBuf,
    settings: GraphSettings,
) {
    let interviewer: Arc<dyn Interviewer> = Arc::new(ConsoleInterviewer::new());
    let human_settings = settings.human.clone();
    let redact_keys = Arc::new(settings.redaction.redact_keys.clone());
    builder
        .register(noop::NoOpOperator::new())
        .register(command::CommandOperator::new(workspace))
        .register(assert_completed::AssertCompletedOperator::new())
        .register(set_context::SetContextOperator::new())
        .register(read_control_file::ReadControlFileOperator::new())
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

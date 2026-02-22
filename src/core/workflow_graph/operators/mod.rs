pub mod assert_completed;
pub mod command;
pub mod noop;
pub mod set_context;

use crate::core::workflow_graph::operator::OperatorRegistryBuilder;
use std::path::PathBuf;

/// Register built-in operators into the supplied builder.
pub fn register_builtins(builder: &mut OperatorRegistryBuilder, workspace: PathBuf) {
    builder
        .register(noop::NoOpOperator::new())
        .register(command::CommandOperator::new(workspace))
        .register(assert_completed::AssertCompletedOperator::new())
        .register(set_context::SetContextOperator::new());
}

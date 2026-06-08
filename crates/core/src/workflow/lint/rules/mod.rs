mod agents;
mod core;
mod expressions;
mod goal_gates;
mod transforms;

use super::WorkflowLintRule;

pub fn built_in_rules() -> Vec<Box<dyn WorkflowLintRule>> {
    let mut rules: Vec<Box<dyn WorkflowLintRule>> = Vec::new();
    rules.extend(core::rules());
    rules.extend(expressions::rules());
    rules.extend(goal_gates::rules());
    rules.extend(agents::rules());
    rules
}

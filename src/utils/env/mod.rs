//! Environment variable management for Newton execution context.

use crate::core::entities::ToolMetadata;
use std::collections::HashMap;

pub struct EnvManager;

impl EnvManager {
    pub fn set_newton_env_vars(
        execution_id: &str,
        iteration_number: usize,
        evaluator: Option<&ToolMetadata>,
        advisor: Option<&ToolMetadata>,
        executor: Option<&ToolMetadata>,
    ) -> HashMap<String, String> {
        EnvContext {
            execution_id,
            iteration_number,
            tools: ToolEnvVars {
                evaluator,
                advisor,
                executor,
            },
        }
        .build_vars()
    }

    pub fn set_environment_variables(vars: &HashMap<String, String>) {
        for (key, value) in vars {
            std::env::set_var(key, value);
        }
    }

    pub fn clear_newton_env_vars() {
        std::env::remove_var("NEWTON_EXECUTION_ID");
        std::env::remove_var("NEWTON_ITERATION_NUMBER");
    }
}

struct EnvContext<'a> {
    execution_id: &'a str,
    iteration_number: usize,
    tools: ToolEnvVars<'a>,
}

impl<'a> EnvContext<'a> {
    fn build_vars(&self) -> HashMap<String, String> {
        let mut env_vars = HashMap::new();
        env_vars.insert(
            format!("NEWTON_EXECUTION_{}", self.execution_id.to_uppercase()),
            self.execution_id.to_string(),
        );
        env_vars.insert(
            "NEWTON_ITERATION_NUMBER".to_string(),
            self.iteration_number.to_string(),
        );

        for (tool_label, metadata) in self.tools.iter() {
            if let Some(metadata) = metadata {
                add_tool_env_vars(&mut env_vars, tool_label, metadata);
            }
        }

        env_vars
    }
}

struct ToolEnvVars<'a> {
    evaluator: Option<&'a ToolMetadata>,
    advisor: Option<&'a ToolMetadata>,
    executor: Option<&'a ToolMetadata>,
}

impl<'a> ToolEnvVars<'a> {
    fn iter(&self) -> [(&'static str, Option<&'a ToolMetadata>); 3] {
        [
            ("evaluator", self.evaluator),
            ("advisor", self.advisor),
            ("executor", self.executor),
        ]
    }
}

fn add_tool_env_vars(
    env_vars: &mut HashMap<String, String>,
    tool_key: &str,
    metadata: &ToolMetadata,
) {
    env_vars.insert("NEWTON_TOOL_TYPE".to_string(), tool_key.to_string());
    env_vars.insert("NEWTON_TOOL_NAME".to_string(), tool_key.to_string());
    for (key, value) in &metadata.environment_variables {
        env_vars.insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests;

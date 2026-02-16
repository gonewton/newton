//! Environment variable management for Newton execution context.

use crate::core::entities::ToolMetadata;
use std::collections::HashMap;

pub struct EnvManager;

/// Context for setting up Newton environment variables
pub struct NewtonEnvContext<'a> {
    pub execution_id: &'a str,
    pub iteration_number: usize,
    pub evaluator: Option<&'a ToolMetadata>,
    pub advisor: Option<&'a ToolMetadata>,
    pub executor: Option<&'a ToolMetadata>,
}

impl EnvManager {
    pub fn set_newton_env_vars(
        execution_id: &str,
        iteration_number: usize,
        evaluator: Option<&ToolMetadata>,
        advisor: Option<&ToolMetadata>,
        executor: Option<&ToolMetadata>,
    ) -> HashMap<String, String> {
        let context = NewtonEnvContext {
            execution_id,
            iteration_number,
            evaluator,
            advisor,
            executor,
        };
        Self::build_env_vars_from_context(&context)
    }

    fn build_env_vars_from_context(context: &NewtonEnvContext) -> HashMap<String, String> {
        let mut env_vars = HashMap::new();

        env_vars.insert(
            format!("NEWTON_EXECUTION_{}", context.execution_id.to_uppercase()),
            context.execution_id.to_string(),
        );
        env_vars.insert(
            "NEWTON_ITERATION_NUMBER".to_string(),
            context.iteration_number.to_string(),
        );

        let tools = [
            (context.evaluator, "evaluator"),
            (context.advisor, "advisor"),
            (context.executor, "executor"),
        ];

        for (tool_opt, tool_name) in tools {
            if let Some(tool) = tool_opt {
                add_tool_env_vars(&mut env_vars, tool_name, tool);
            }
        }

        env_vars
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

fn add_tool_env_vars(env_vars: &mut HashMap<String, String>, tool_name: &str, tool: &ToolMetadata) {
    env_vars.insert("NEWTON_TOOL_TYPE".to_string(), tool_name.to_string());
    env_vars.insert("NEWTON_TOOL_NAME".to_string(), tool_name.to_string());
    for (key, value) in &tool.environment_variables {
        env_vars.insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests;

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
        let mut env_vars = HashMap::new();

        env_vars.insert(
            format!("NEWTON_EXECUTION_{}", execution_id.to_uppercase()),
            execution_id.to_string(),
        );
        env_vars.insert(
            format!("NEWTON_ITERATION_{}", iteration_number.to_string()),
            iteration_number.to_string(),
        );

        if let Some(evaluator) = evaluator {
            env_vars.insert("NEWTON_TOOL_TYPE".to_string(), "evaluator".to_string());
            env_vars.insert("NEWTON_TOOL_NAME".to_string(), "evaluator".to_string());
            for (key, value) in &evaluator.environment_variables {
                env_vars.insert(key.clone(), value.clone());
            }
        }

        if let Some(advisor) = advisor {
            env_vars.insert("NEWTON_TOOL_TYPE".to_string(), "advisor".to_string());
            env_vars.insert("NEWTON_TOOL_NAME".to_string(), "advisor".to_string());
            for (key, value) in &advisor.environment_variables {
                env_vars.insert(key.clone(), value.clone());
            }
        }

        if let Some(executor) = executor {
            env_vars.insert("NEWTON_TOOL_TYPE".to_string(), "executor".to_string());
            env_vars.insert("NEWTON_TOOL_NAME".to_string(), "executor".to_string());
            for (key, value) in &executor.environment_variables {
                env_vars.insert(key.clone(), value.clone());
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
        if let Ok(exe_id) = std::env::var("NEWTON_EXECUTION_ID") {
            std::env::remove_var(&format!("NEWTON_EXECUTION_{}", exe_id.to_uppercase()));
            std::env::remove_var(&format!(
                "NEWTON_ITERATION_{}",
                std::env::var("NEWTON_ITERATION_NUMBER").unwrap_or_default()
            ));
            std::env::remove_var("NEWTON_EXECUTION_ID");
            std::env::remove_var("NEWTON_ITERATION_NUMBER");
        }
    }
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main Newton configuration loaded from newton.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewtonConfig {
    /// Project configuration
    #[serde(default)]
    pub project: ProjectConfig,

    /// Executor configuration
    #[serde(default)]
    pub executor: ExecutorConfig,

    /// Evaluator configuration
    #[serde(default)]
    pub evaluator: EvaluatorConfig,

    /// Advisor configuration
    #[serde(default)]
    pub advisor: AdvisorConfig,

    /// Context configuration
    #[serde(default)]
    pub context: ContextConfig,

    /// Promise configuration
    #[serde(default)]
    pub promise: PromiseConfig,
}

/// Project configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name
    pub name: String,

    /// Project template
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

/// Executor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// Coding agent to use
    #[serde(default = "default_coding_agent")]
    pub coding_agent: String,

    /// Coding agent model
    #[serde(default = "default_coding_agent_model")]
    pub coding_agent_model: String,

    /// Auto commit changes
    #[serde(default)]
    pub auto_commit: bool,
}

/// Evaluator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorConfig {
    /// Test command to run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_command: Option<String>,

    /// Score threshold for success
    #[serde(default = "default_score_threshold")]
    pub score_threshold: f64,
}

/// Advisor configuration (placeholder for future expansion)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdvisorConfig {
    // Advisor-specific configuration fields will be added here
}

/// Context configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Clear context after use
    #[serde(default = "default_clear_after_use")]
    pub clear_after_use: bool,

    /// Context file path
    #[serde(default = "default_context_file")]
    pub file: PathBuf,
}

/// Promise configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromiseConfig {
    /// Promise file path
    #[serde(default = "default_promise_file")]
    pub file: PathBuf,
}

// Default functions
fn default_coding_agent() -> String {
    "opencode".to_string()
}

fn default_coding_agent_model() -> String {
    "zai-coding-plan/glm-4.7".to_string()
}

fn default_score_threshold() -> f64 {
    95.0
}

fn default_clear_after_use() -> bool {
    true
}

fn default_context_file() -> PathBuf {
    PathBuf::from(".newton/state/context.md")
}

fn default_promise_file() -> PathBuf {
    PathBuf::from(".newton/state/promise.txt")
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            name: "newton-project".to_string(),
            template: None,
        }
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        ExecutorConfig {
            coding_agent: default_coding_agent(),
            coding_agent_model: default_coding_agent_model(),
            auto_commit: false,
        }
    }
}

impl Default for EvaluatorConfig {
    fn default() -> Self {
        EvaluatorConfig {
            test_command: None,
            score_threshold: default_score_threshold(),
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        ContextConfig {
            clear_after_use: default_clear_after_use(),
            file: default_context_file(),
        }
    }
}

impl Default for PromiseConfig {
    fn default() -> Self {
        PromiseConfig {
            file: default_promise_file(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn test_config_defaults() {
        let config = NewtonConfig::default();
        assert_debug_snapshot!(config);

        assert_eq!(config.project.name, "newton-project");
        assert_eq!(config.executor.coding_agent, "opencode");
        assert_eq!(
            config.executor.coding_agent_model,
            "zai-coding-plan/glm-4.7"
        );
        assert!(!config.executor.auto_commit);
        assert_eq!(config.evaluator.score_threshold, 95.0);
        assert!(config.context.clear_after_use);
        assert_eq!(
            config.context.file,
            PathBuf::from(".newton/state/context.md")
        );
        assert_eq!(
            config.promise.file,
            PathBuf::from(".newton/state/promise.txt")
        );
    }

    #[test]
    fn test_deserialize_minimal_config() {
        let toml = r#"
[project]
name = "my-project"
"#;

        let config: NewtonConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.project.name, "my-project");
        assert_eq!(config.executor.coding_agent, "opencode"); // Should use default
    }

    #[test]
    fn test_deserialize_full_config() {
        let toml = r#"
[project]
name = "my-project"
template = "coding-project"

[executor]
coding_agent = "custom-agent"
coding_agent_model = "custom-model"
auto_commit = true

[evaluator]
test_command = "./scripts/run-tests.sh"
score_threshold = 90.0

[context]
clear_after_use = false
file = ".custom/context.md"

[promise]
file = ".custom/promise.txt"
"#;

        let config: NewtonConfig = toml::from_str(toml).unwrap();
        assert_debug_snapshot!(config);

        assert_eq!(config.project.name, "my-project");
        assert_eq!(config.project.template, Some("coding-project".to_string()));
        assert_eq!(config.executor.coding_agent, "custom-agent");
        assert_eq!(config.executor.coding_agent_model, "custom-model");
        assert!(config.executor.auto_commit);
        assert_eq!(
            config.evaluator.test_command,
            Some("./scripts/run-tests.sh".to_string())
        );
        assert_eq!(config.evaluator.score_threshold, 90.0);
        assert!(!config.context.clear_after_use);
        assert_eq!(config.context.file, PathBuf::from(".custom/context.md"));
        assert_eq!(config.promise.file, PathBuf::from(".custom/promise.txt"));
    }

    #[test]
    fn test_deserialize_with_optional_sections_missing() {
        let toml = r#"
[project]
name = "minimal-project"

[executor]
auto_commit = true
"#;

        let config: NewtonConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.project.name, "minimal-project");
        assert_eq!(config.executor.coding_agent, "opencode"); // Default value
        assert_eq!(
            config.executor.coding_agent_model,
            "zai-coding-plan/glm-4.7"
        ); // Default value
        assert!(config.executor.auto_commit);
        assert!(config.evaluator.test_command.is_none()); // Optional field missing
        assert_eq!(config.evaluator.score_threshold, 95.0); // Default value
    }
}

pub mod loader;
pub mod validation;

pub use loader::ConfigLoader;
pub use validation::ConfigValidator;

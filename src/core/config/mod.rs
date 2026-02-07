mod loader;
mod validation;

pub use loader::ConfigLoader;
pub use validation::ConfigValidator;

use serde::{Deserialize, Serialize};

/// Main Newton configuration loaded from newton.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewtonConfig {
    /// Evaluator command (optional, can be overridden via CLI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_cmd: Option<String>,

    /// Advisor command (optional, can be overridden via CLI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisor_cmd: Option<String>,

    /// Executor command (optional, can be overridden via CLI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_cmd: Option<String>,

    /// Control file path (default: "newton_control.json")
    #[serde(default = "default_control_file")]
    pub control_file: String,

    /// Branch configuration
    #[serde(default)]
    pub branch: BranchConfig,

    /// Git/GH configuration
    #[serde(default)]
    pub git: GitConfig,
}

/// Branch management configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BranchConfig {
    /// Enable branch creation from goal
    #[serde(default)]
    pub create_from_goal: bool,

    /// Command to generate branch name from goal
    /// Receives NEWTON_GOAL and NEWTON_STATE_DIR env vars
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_namer_cmd: Option<String>,
}

/// Git/GitHub integration configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitConfig {
    /// Restore original branch after completion
    #[serde(default)]
    pub restore_original_branch: bool,

    /// Create PR on successful completion
    #[serde(default)]
    pub create_pr_on_success: bool,
}

fn default_control_file() -> String {
    "newton_control.json".to_string()
}

impl Default for NewtonConfig {
    fn default() -> Self {
        NewtonConfig {
            evaluator_cmd: None,
            advisor_cmd: None,
            executor_cmd: None,
            control_file: default_control_file(),
            branch: BranchConfig::default(),
            git: GitConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = NewtonConfig::default();
        assert_eq!(config.control_file, "newton_control.json");
        assert!(!config.branch.create_from_goal);
        assert!(!config.git.restore_original_branch);
        assert!(!config.git.create_pr_on_success);
    }

    #[test]
    fn test_deserialize_minimal_config() {
        let toml = r#""#;
        let config: NewtonConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.control_file, "newton_control.json");
    }

    #[test]
    fn test_deserialize_full_config() {
        let toml = r#"
            evaluator_cmd = "eval.sh"
            advisor_cmd = "advise.sh"
            executor_cmd = "exec.sh"
            control_file = "custom_control.json"

            [branch]
            create_from_goal = true
            branch_namer_cmd = "namer.sh"

            [git]
            restore_original_branch = true
            create_pr_on_success = true
        "#;
        let config: NewtonConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.evaluator_cmd, Some("eval.sh".to_string()));
        assert_eq!(config.advisor_cmd, Some("advise.sh".to_string()));
        assert_eq!(config.executor_cmd, Some("exec.sh".to_string()));
        assert_eq!(config.control_file, "custom_control.json");
        assert!(config.branch.create_from_goal);
        assert_eq!(config.branch.branch_namer_cmd, Some("namer.sh".to_string()));
        assert!(config.git.restore_original_branch);
        assert!(config.git.create_pr_on_success);
    }
}

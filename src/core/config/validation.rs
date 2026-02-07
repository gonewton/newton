#![allow(clippy::result_large_err)]

use super::NewtonConfig;
use crate::core::error::AppError;

pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate configuration rules
    pub fn validate(config: &NewtonConfig) -> Result<(), AppError> {
        // Validate control_file is not empty
        if config.control_file.trim().is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "control_file cannot be empty",
            ));
        }

        // If create_from_goal is enabled, require branch_namer_cmd
        if config.branch.create_from_goal && config.branch.branch_namer_cmd.is_none() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "branch.branch_namer_cmd is required when branch.create_from_goal is true",
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::NewtonConfig;

    #[test]
    fn test_validate_valid_config() {
        let config = NewtonConfig::default();
        assert!(ConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn test_validate_empty_control_file() {
        let config = NewtonConfig {
            control_file: "".to_string(),
            ..Default::default()
        };
        assert!(ConfigValidator::validate(&config).is_err());
    }

    #[test]
    fn test_validate_branch_from_goal_without_namer() {
        let mut config = NewtonConfig::default();
        config.branch.create_from_goal = true;
        config.branch.branch_namer_cmd = None;

        let result = ConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("branch_namer_cmd"));
    }

    #[test]
    fn test_validate_branch_from_goal_with_namer() {
        let mut config = NewtonConfig::default();
        config.branch.create_from_goal = true;
        config.branch.branch_namer_cmd = Some("namer.sh".to_string());

        assert!(ConfigValidator::validate(&config).is_ok());
    }
}

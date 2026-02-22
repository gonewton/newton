use super::NewtonConfig;
use crate::core::error::AppError;

pub struct ConfigValidator;

impl ConfigValidator {
    #[allow(clippy::result_large_err)] // AppError carries validation context; we keep the concrete type for clarity.
    pub fn validate(config: &NewtonConfig) -> Result<(), AppError> {
        if config.project.name.is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Project name cannot be empty".to_string(),
            ));
        }

        if config.evaluator.score_threshold < 0.0 || config.evaluator.score_threshold > 100.0 {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Score threshold must be between 0.0 and 100.0".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let config = NewtonConfig::default();
        assert!(ConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn test_validate_empty_project_name() {
        let mut config = NewtonConfig::default();
        config.project.name = "".to_string();
        assert!(ConfigValidator::validate(&config).is_err());
    }

    #[test]
    fn test_validate_invalid_score_threshold() {
        let mut config = NewtonConfig::default();
        config.evaluator.score_threshold = 150.0;
        assert!(ConfigValidator::validate(&config).is_err());
    }
}

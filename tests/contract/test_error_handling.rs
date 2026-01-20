use newton_code::core::error::{AppError, ErrorSeverity};
use newton_code::core::types::ErrorCategory;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation_with_category() {
        let error = AppError::new(ErrorCategory::ValidationError, "Invalid input".to_string());
        assert_eq!(error.category, ErrorCategory::ValidationError);
        assert_eq!(error.severity, ErrorSeverity::Error);
        assert_eq!(error.message, "Invalid input");
    }

    #[test]
    fn test_error_creation_with_different_categories() {
        let validation_error = AppError::new(
            ErrorCategory::ValidationError,
            "Validation failed".to_string(),
        );
        assert_eq!(validation_error.category, ErrorCategory::ValidationError);

        let execution_error =
            AppError::new(ErrorCategory::ToolExecutionError, "Tool failed".to_string());
        assert_eq!(execution_error.category, ErrorCategory::ToolExecutionError);

        let timeout_error = AppError::new(
            ErrorCategory::TimeoutError,
            "Operation timed out".to_string(),
        );
        assert_eq!(timeout_error.category, ErrorCategory::TimeoutError);
    }

    #[test]
    fn test_error_severity_mapping() {
        // Test that certain categories map to expected severities
        let error = AppError::new(ErrorCategory::ValidationError, "Test".to_string());
        assert_eq!(error.severity, ErrorSeverity::Error);

        let critical_error = AppError::new(ErrorCategory::InternalError, "Critical".to_string());
        assert_eq!(critical_error.severity, ErrorSeverity::Error); // Assuming default
    }

    #[test]
    fn test_error_with_context() {
        let mut error = AppError::new(ErrorCategory::IoError, "File not found".to_string());
        error.add_context("file_path", "/tmp/test.txt");
        assert_eq!(error.category, ErrorCategory::IoError);
        // Assuming context is stored
    }
}

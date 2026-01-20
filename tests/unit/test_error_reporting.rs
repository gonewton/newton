use newton_code::core::error::{AppError, DefaultErrorReporter, ErrorReporter, ErrorSeverity};
use newton_code::core::types::ErrorCategory;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_error_reporter_report_error() {
        let reporter = DefaultErrorReporter;
        let error = AppError::new(ErrorCategory::ValidationError, "Test error".to_string());
        // Since report_error prints to stdout, we can't easily test output
        // In a real implementation, we might use a mock or capture output
        reporter.report_error(&error);
        // Assert no panic
    }

    #[test]
    fn test_default_error_reporter_report_warning() {
        let reporter = DefaultErrorReporter;
        reporter.report_warning("Test warning".to_string(), Some("context".to_string()));
        // Assert no panic
    }

    #[test]
    fn test_default_error_reporter_report_info() {
        let reporter = DefaultErrorReporter;
        reporter.report_info("Test info");
        // Assert no panic
    }

    #[test]
    fn test_error_reporter_trait() {
        // Test that DefaultErrorReporter implements ErrorReporter
        let reporter: Box<dyn ErrorReporter> = Box::new(DefaultErrorReporter);
        let error = AppError::new(ErrorCategory::InternalError, "Trait test".to_string());
        reporter.report_error(&error);
        // Assert no panic
    }
}

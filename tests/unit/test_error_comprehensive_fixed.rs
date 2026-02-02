use newton::core::error::{AppError, DefaultErrorReporter, ErrorReporter};
use newton::core::types::{ErrorCategory, ErrorSeverity};
use std::error::Error;

#[test]
fn test_error_creation_all_categories() {
    let categories = vec![
        ErrorCategory::ValidationError,
        ErrorCategory::ToolExecutionError,
        ErrorCategory::TimeoutError,
        ErrorCategory::ResourceError,
        ErrorCategory::WorkspaceError,
        ErrorCategory::IterationError,
        ErrorCategory::SerializationError,
        ErrorCategory::IoError,
        ErrorCategory::ArtifactError,
        ErrorCategory::InternalError,
        ErrorCategory::Unknown,
    ];
    
    for category in categories {
        let error = AppError::new(category, "test message");
        assert_eq!(error.category, category);
        assert_eq!(error.message, "test message");
        assert_eq!(error.context.len(), 0);
        assert_eq!(error.recovery_suggestions.len(), 0);
        assert!(error.occurred_at <= chrono::Utc::now());
        assert!(error.stack_trace.is_none());
        assert!(error.source.is_none());
    }
}

#[test]
fn test_error_severity_mapping() {
    let test_cases = vec![
        (ErrorCategory::ValidationError, ErrorSeverity::Error),
        (ErrorCategory::ToolExecutionError, ErrorSeverity::Error),
        (ErrorCategory::TimeoutError, ErrorSeverity::Error),
        (ErrorCategory::ResourceError, ErrorSeverity::Error),
        (ErrorCategory::WorkspaceError, ErrorSeverity::Error),
        (ErrorCategory::IterationError, ErrorSeverity::Error),
        (ErrorCategory::SerializationError, ErrorSeverity::Error),
        (ErrorCategory::IoError, ErrorSeverity::Error),
        (ErrorCategory::ArtifactError, ErrorSeverity::Error),
        (ErrorCategory::InternalError, ErrorSeverity::Error),
        (ErrorCategory::Unknown, ErrorSeverity::Info),
    ];
    
    for (category, expected_severity) in test_cases {
        let error = AppError::new(category, "test");
        assert_eq!(error.severity(), expected_severity);
    }
}

#[test]
fn test_error_builder_pattern() {
    let error = AppError::new(ErrorCategory::ValidationError, "validation failed")
        .with_context("user input")
        .with_code("VAL-001");
    
    assert_eq!(error.code, "VAL-001");
    assert!(error.context.contains_key("context"));
    assert_eq!(error.context.get("context"), Some(&"user input".to_string()));
}

#[test]
fn test_error_add_context() {
    let mut error = AppError::new(ErrorCategory::ToolExecutionError, "tool failed");
    
    error.add_context("tool_name", "validator");
    error.add_context("iteration", "5");
    
    assert_eq!(error.context.get("tool_name"), Some(&"validator".to_string()));
    assert_eq!(error.context.get("iteration"), Some(&"5".to_string()));
    assert_eq!(error.context.len(), 2);
}

#[test]
fn test_error_display() {
    let mut error = AppError::new(ErrorCategory::ValidationError, "invalid input")
        .with_code("VAL-001");
    error.add_context("field", "email");
    
    let display = format!("{}", error);
    assert!(display.contains("VAL-001"));
    assert!(display.contains("ValidationError"));
    assert!(display.contains("invalid input"));
    assert!(display.contains("email"));
}

#[test]
fn test_error_source_access() {
    let source_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let app_error = AppError::from(source_error);
    
    // Test that we can access the source
    let source = app_error.source;
    assert!(source.is_some());
}

#[test]
fn test_error_from_anyhow() {
    let anyhow_error = anyhow::anyhow!("anyhow error message");
    let app_error = AppError::from(anyhow_error);
    
    assert_eq!(app_error.category, ErrorCategory::InternalError);
    assert_eq!(app_error.message, "anyhow error message");
    assert_eq!(app_error.code, "ANYHOW_ERROR");
    assert_eq!(app_error.severity(), ErrorSeverity::Error);
}

#[test]
fn test_error_from_io_error() {
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let app_error = AppError::from(io_error);
    
    assert_eq!(app_error.category, ErrorCategory::IoError);
    assert_eq!(app_error.message, "file not found");
    assert_eq!(app_error.code, "IO_ERROR");
    assert_eq!(app_error.severity(), ErrorSeverity::Error);
}

#[test]
fn test_error_trait_impl() {
    let error = AppError::new(ErrorCategory::ValidationError, "test error");
    
    // Test that Error trait is implemented
    let source = error.source();
    assert!(source.is_none());
    
    // Test that it can be used as a dyn Error
    let boxed: Box<dyn Error> = Box::new(error);
    assert!(boxed.to_string().contains("test error"));
}

#[test]
fn test_error_reporter() {
    let reporter = DefaultErrorReporter::new();
    
    let error = AppError::new(ErrorCategory::ValidationError, "test error");
    
    // These methods should not panic
    reporter.report_error(&error);
    reporter.report_warning("test warning", Some("context".to_string()));
    reporter.report_info("test info");
    reporter.report_debug("test debug");
}

#[test]
fn test_error_recovery_suggestions() {
    // When created from IO error, it should have recovery suggestions
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
    let app_error = AppError::from(io_error);
    assert_eq!(app_error.recovery_suggestions.len(), 1);
    assert_eq!(app_error.recovery_suggestions[0], "Check file permissions and paths");
}

#[test]
fn test_error_complex_scenario() {
    let mut error = AppError::new(ErrorCategory::ToolExecutionError, "command failed")
        .with_context("command")
        .with_code("TOOL-001");
    
    error.add_context("exit_code", "1");
    error.add_context("duration_ms", "5000");
    
    let expected_keys = vec!["context", "command", "exit_code", "duration_ms"];
    for key in expected_keys {
        assert!(error.context.contains_key(key));
    }
    
    assert_eq!(error.code, "TOOL-001");
    assert_eq!(error.category, ErrorCategory::ToolExecutionError);
}

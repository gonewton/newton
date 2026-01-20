use crate::core::types::ErrorCategory;
use std::fmt;

#[derive(Debug)]
pub struct AppError {
    pub category: ErrorCategory,
    pub message: String,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    pub context: Option<String>,
    pub code: String,
}

impl AppError {
    pub fn new<T: Into<String>>(category: ErrorCategory, message: T) -> Self {
        AppError {
            category,
            message: message.into(),
            source: None,
            context: None,
            code: format!("ERR-{}", uuid::Uuid::new_v4()),
        }
    }

    pub fn with_source<T: Into<String>>(
        category: ErrorCategory,
        message: T,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        let mut error = AppError::new(category, message);
        error.source = Some(source);
        error
    }

    pub fn with_context<T: Into<String>>(mut self, context: T) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_code<T: Into<String>>(mut self, code: T) -> Self {
        self.code = code.into();
        self
    }

    pub fn severity(&self) -> ErrorSeverity {
        match self.category {
            ErrorCategory::ValidationError => ErrorSeverity::Error,
            ErrorCategory::ToolExecutionError => ErrorSeverity::Error,
            ErrorCategory::TimeoutError => ErrorSeverity::Error,
            ErrorCategory::ResourceError => ErrorSeverity::Error,
            ErrorCategory::WorkspaceError => ErrorSeverity::Error,
            ErrorCategory::IterationError => ErrorSeverity::Error,
            ErrorCategory::SerializationError => ErrorSeverity::Error,
            ErrorCategory::IoError => ErrorSeverity::Error,
            ErrorCategory::InternalError => ErrorSeverity::Error,
            ErrorCategory::Unknown => ErrorSeverity::Info,
        }
    }
}

#[derive(Debug)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Debug,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.category, self.message)?;
        if let Some(ref context) = self.context {
            write!(f, " (Context: {})", context)?;
        }
        if let Some(ref source) = self.source {
            write!(f, "\nCaused by: {}", source)?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError {
            id: uuid::Uuid::new_v4(),
            category: ErrorCategory::InternalError,
            severity: ErrorSeverity::Error,
            code: "ANYHOW_ERROR".to_string(),
            message: e.to_string(),
            context: Default::default(),
            recovery_suggestions: vec!["Check the error details".to_string()],
            occurred_at: chrono::Utc::now(),
            stack_trace: None,
        }
    }
}

impl AppError {
    pub fn new(category: ErrorCategory, message: String) -> Self {
        let severity = match category {
            ErrorCategory::ValidationError
            | ErrorCategory::ToolExecutionError
            | ErrorCategory::TimeoutError
            | ErrorCategory::ResourceError
            | ErrorCategory::WorkspaceError
            | ErrorCategory::IterationError
            | ErrorCategory::SerializationError
            | ErrorCategory::IoError => ErrorSeverity::Error,
            ErrorCategory::InternalError => ErrorSeverity::High,
            ErrorCategory::Unknown => ErrorSeverity::Medium,
        };
        AppError {
            id: uuid::Uuid::new_v4(),
            category,
            severity,
            code: format!("{:?}", category)
                .to_uppercase()
                .replace("ERROR", "")
                .replace("CATEGORY::", ""),
            message,
            context: std::collections::HashMap::new(),
            recovery_suggestions: vec!["Check logs for details".to_string()],
            occurred_at: chrono::Utc::now(),
            stack_trace: None,
        }
    }

    pub fn add_context(&mut self, key: &str, value: &str) {
        self.context.insert(key.to_string(), value.to_string());
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError {
            id: uuid::Uuid::new_v4(),
            category: ErrorCategory::IoError,
            severity: ErrorSeverity::Error,
            code: "IO_ERROR".to_string(),
            message: e.to_string(),
            context: Default::default(),
            recovery_suggestions: vec!["Check file permissions and paths".to_string()],
            occurred_at: chrono::Utc::now(),
            stack_trace: None,
        }
    }
}

pub trait ErrorReporter {
    fn report_error(&self, error: &AppError);
    fn report_warning(&self, message: &str, context: Option<String>);
    fn report_info(&self, message: &str);
    fn report_debug(&self, message: &str);
}

pub struct DefaultErrorReporter;

impl DefaultErrorReporter {
    pub fn new() -> Self {
        DefaultErrorReporter
    }
}

impl ErrorReporter for DefaultErrorReporter {
    fn report_error(&self, error: &AppError) {
        eprintln!("[ERROR] {}: {}", error.code, error.message);
        if let Some(ref context) = error.context {
            eprintln!("  Context: {}", context);
        }
        if let Some(ref source) = error.source {
            eprintln!("  Caused by: {}", source);
        }
    }

    fn report_warning(&self, message: &str, context: Option<String>) {
        eprintln!("[WARNING] {}", message);
        if let Some(ref ctx) = context {
            eprintln!("  Context: {}", ctx);
        }
    }

    fn report_info(&self, message: &str) {
        println!("[INFO] {}", message);
    }

    fn report_debug(&self, message: &str) {
        println!("[DEBUG] {}", message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = AppError::new(ErrorCategory::ValidationError, "test error");
        assert_eq!(error.category, ErrorCategory::ValidationError);
        assert_eq!(error.message, "test error");
    }

    #[test]
    fn test_error_with_context() {
        let mut error = AppError::new(ErrorCategory::ToolExecutionError, "tool failed");
        error = error.with_context("iteration 5");
        assert_eq!(error.context, Some("iteration 5".to_string()));
    }

    #[test]
    fn test_error_with_code() {
        let mut error = AppError::new(ErrorCategory::InternalError, "system error");
        error = error.with_code("TEST-001");
        assert_eq!(error.code, "TEST-001");
    }

    #[test]
    fn test_error_severity() {
        let error = AppError::new(ErrorCategory::ValidationError, "test");
        assert_eq!(error.severity(), ErrorSeverity::Error);
    }
}

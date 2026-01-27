use crate::core::types::{ErrorCategory, ErrorSeverity};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug)]
pub struct AppError {
    pub category: ErrorCategory,
    pub severity: ErrorSeverity,
    pub code: String,
    pub message: String,
    pub context: HashMap<String, String>,
    pub recovery_suggestions: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    pub stack_trace: Option<String>,
    pub source: Option<anyhow::Error>,
}

impl AppError {
    pub fn new<T: Into<String>>(category: ErrorCategory, message: T) -> Self {
        let severity = match category {
            ErrorCategory::ValidationError
            | ErrorCategory::ToolExecutionError
            | ErrorCategory::TimeoutError
            | ErrorCategory::ResourceError
            | ErrorCategory::WorkspaceError
            | ErrorCategory::IterationError
            | ErrorCategory::SerializationError
            | ErrorCategory::IoError
            | ErrorCategory::ArtifactError => ErrorSeverity::Error,
            ErrorCategory::InternalError => ErrorSeverity::Error,
            ErrorCategory::Unknown => ErrorSeverity::Info,
        };
        AppError {
            category,
            severity,
            code: format!("ERR-{}", uuid::Uuid::new_v4()),
            message: message.into(),
            context: HashMap::new(),
            recovery_suggestions: vec![],
            occurred_at: chrono::Utc::now(),
            stack_trace: None,
            source: None,
        }
    }

    pub fn with_source<T: Into<String>>(
        category: ErrorCategory,
        message: T,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        let mut error = AppError::new(category, message);
        error.source = Some(anyhow::anyhow!(source));
        error
    }

    pub fn with_context<T: Into<String>>(mut self, context: T) -> Self {
        self.context.insert("context".to_string(), context.into());
        self
    }

    pub fn with_code<T: Into<String>>(mut self, code: T) -> Self {
        self.code = code.into();
        self
    }

    pub fn severity(&self) -> ErrorSeverity {
        self.severity
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.category, self.message)?;
        if !self.context.is_empty() {
            write!(f, " (Context: {:?})", self.context)?;
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
            category: ErrorCategory::InternalError,
            severity: ErrorSeverity::Error,
            code: "ANYHOW_ERROR".to_string(),
            message: e.to_string(),
            context: HashMap::new(),
            recovery_suggestions: vec!["Check the error details".to_string()],
            occurred_at: Utc::now(),
            stack_trace: None,
            source: Some(e),
        }
    }
}

impl AppError {
    pub fn add_context(&mut self, key: &str, value: &str) {
        self.context.insert(key.to_string(), value.to_string());
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError {
            category: ErrorCategory::IoError,
            severity: ErrorSeverity::Error,
            code: "IO_ERROR".to_string(),
            message: e.to_string(),
            context: HashMap::new(),
            recovery_suggestions: vec!["Check file permissions and paths".to_string()],
            occurred_at: Utc::now(),
            stack_trace: None,
            source: Some(anyhow::anyhow!(e)),
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

impl Default for DefaultErrorReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorReporter for DefaultErrorReporter {
    fn report_error(&self, error: &AppError) {
        eprintln!("[ERROR] {}: {}", error.code, error.message);
        if !error.context.is_empty() {
            eprintln!("  Context: {:?}", error.context);
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
        error.add_context("context", "iteration 5");
        assert_eq!(
            error.context.get("context"),
            Some(&"iteration 5".to_string())
        );
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

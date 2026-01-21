use crate::core::entities::*;
use crate::core::entities::{Workspace, WorkspaceConfiguration};
use crate::core::error::{AppError, ErrorReporter};
use crate::core::types::WorkspaceValidatorTrait;

pub struct WorkspaceManager {
    validator: Box<dyn WorkspaceValidatorTrait>,
    reporter: Box<dyn ErrorReporter>,
}

impl WorkspaceManager {
    pub fn new(
        validator: Box<dyn WorkspaceValidatorTrait>,
        reporter: Box<dyn ErrorReporter>,
    ) -> Self {
        WorkspaceManager {
            validator,
            reporter,
        }
    }

    pub fn initialize_workspace(&self, path: &std::path::PathBuf) -> Result<Workspace, AppError> {
        self.reporter
            .report_info(&format!("Initializing workspace at: {:?}", path));

        self.validator.validate_path(path)?;

        let workspace = Workspace {
            id: uuid::Uuid::new_v4().to_string(),
            name: "New Workspace".to_string(),
            description: Some("Auto-generated workspace".to_string()),
            path: path.clone(),
            configuration: WorkspaceConfiguration {
                name: "New Workspace".to_string(),
                description: Some("Auto-generated workspace".to_string()),
                template_id: None,
                parameters: Vec::new(),
                settings: Default::default(),
            },
            template_id: None,
            status: WorkspaceStatus::Valid,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: None,
            last_used: None,
        };

        self.reporter
            .report_info("Workspace initialized successfully");
        Ok(workspace)
    }

    pub fn validate_workspace(&self, path: &std::path::PathBuf) -> Result<(), AppError> {
        self.validator.validate_path(path)?;
        self.validator.validate_structure(path)?;
        self.validator.validate_configuration(path)?;
        Ok(())
    }
}

pub struct TestValidator;

impl TestValidator {
    pub fn new() -> Self {
        Self
    }
}

impl WorkspaceValidatorTrait for TestValidator {
    fn validate_path(
        &self,
        path: &std::path::Path,
    ) -> Result<(), crate::core::types::WorkspaceValidationError> {
        if !path.exists() {
            return Err(crate::core::types::WorkspaceValidationError::PathNotFound {
                path: path.to_string_lossy().to_string(),
            });
        }

        if !path.is_dir() {
            return Err(
                crate::core::types::WorkspaceValidationError::PathNotDirectory {
                    path: path.to_string_lossy().to_string(),
                },
            );
        }

        Ok(())
    }

    fn validate_structure(
        &self,
        path: &std::path::Path,
    ) -> Result<(), crate::core::types::WorkspaceValidationError> {
        if path.join("config.toml").exists() {
            Ok(())
        } else {
            Err(
                crate::core::types::WorkspaceValidationError::ConfigFileMissing {
                    file: "config.toml".to_string(),
                },
            )
        }
    }

    fn validate_configuration(
        &self,
        path: &std::path::Path,
    ) -> Result<(), crate::core::types::WorkspaceValidationError> {
        if !path.exists() {
            return Err(crate::core::types::WorkspaceValidationError::PathNotFound {
                path: path.to_string_lossy().to_string(),
            });
        }
        Ok(())
    }

    fn is_locked(&self, path: &std::path::Path) -> bool {
        false
    }
}

pub struct TestReporterImpl;

impl TestReporterImpl {
    pub fn new() -> Self {
        Self
    }
}

impl ErrorReporter for TestReporterImpl {
    fn report_error(&self, error: &AppError) {
        println!("[ERROR] {}: {}", error.code, error.message);
    }

    fn report_warning(&self, message: &str, context: Option<String>) {
        println!("[WARNING] {}", message);
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
    fn test_workspace_manager_creation() {
        let manager = WorkspaceManager::new(
            Box::new(TestValidator::new()),
            Box::new(TestReporterImpl::new()),
        );
        assert!(true);
    }

    #[test]
    fn test_test_validator_creation() {
        let validator = TestValidator::new();
        assert!(true);
    }

    #[test]
    fn test_test_reporter_creation() {
        let reporter = TestReporterImpl::new();
        assert!(true);
    }
}

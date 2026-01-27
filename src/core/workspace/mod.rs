use crate::core::error::{AppError, ErrorReporter};

pub struct WorkspaceManager {
    _reporter: Box<dyn ErrorReporter>,
}

impl WorkspaceManager {
    pub fn new(reporter: Box<dyn ErrorReporter>) -> Self {
        WorkspaceManager {
            _reporter: reporter,
        }
    }

    pub fn new_default() -> Self {
        WorkspaceManager {
            _reporter: Box::new(crate::core::error::DefaultErrorReporter),
        }
    }

    pub fn validate_path(&self, path: &std::path::Path) -> Result<(), AppError> {
        if !path.exists() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                format!("Path not found: {}", path.display()),
            ));
        }

        if !path.is_dir() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                format!("Path is not a directory: {}", path.display()),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_manager_creation() {
        let _manager = WorkspaceManager::new_default();
    }

    #[test]
    fn test_validate_path_valid_directory() {
        let manager = WorkspaceManager::new_default();
        let temp_dir = tempfile::TempDir::new().unwrap();
        assert!(manager.validate_path(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_validate_path_nonexistent() {
        let manager = WorkspaceManager::new_default();
        let nonexistent_path = std::path::Path::new("/nonexistent/path");
        assert!(manager.validate_path(nonexistent_path).is_err());
    }

    #[test]
    fn test_validate_path_file() {
        let manager = WorkspaceManager::new_default();
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        assert!(manager.validate_path(temp_file.path()).is_err());
    }
}

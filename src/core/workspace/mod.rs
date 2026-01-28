use crate::core::error::AppError;

pub fn validate_path(path: &std::path::Path) -> Result<(), AppError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_valid_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        assert!(validate_path(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_validate_path_nonexistent() {
        let nonexistent_path = std::path::Path::new("/nonexistent/path");
        assert!(validate_path(nonexistent_path).is_err());
    }

    #[test]
    fn test_validate_path_file() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        assert!(validate_path(temp_file.path()).is_err());
    }
}

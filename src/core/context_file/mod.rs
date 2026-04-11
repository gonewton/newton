#![allow(clippy::result_large_err)] // Context manager returns AppError so callers can log detailed IO issues without trait objects.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::fs;
use std::path::Path;

/// Simple manager for the Newton context file.
pub struct ContextManager;

impl ContextManager {
    /// Clear the context file and ensure it exists with a header.
    pub fn clear_context(context_file: &Path) -> Result<(), AppError> {
        if let Some(parent) = context_file.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to create context directory {}: {}",
                        parent.display(),
                        e
                    ),
                )
            })?;
        }

        fs::write(context_file, "# Newton Loop Context\n\n").map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to write context file {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn clears_context_file() {
        let tmp = TempDir::new().unwrap();
        let context_path = tmp.path().join(".newton/state/context.md");
        ContextManager::clear_context(&context_path).unwrap();
        let content = fs::read_to_string(&context_path).unwrap();
        assert!(content.starts_with("# Newton Loop Context"));
    }
}

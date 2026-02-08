#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::fs;
use std::path::Path;

/// Parses and persists promise markers emitted by executor tools.
pub struct PromiseDetector;

impl PromiseDetector {
    /// Search executor text for the first `<promise>...</promise>` entry that is complete.
    pub fn detect_promise(executor_output: &str) -> Option<String> {
        let mut search_start = 0;
        while let Some(start) = executor_output[search_start..].find("<promise>") {
            let start_index = search_start + start + "<promise>".len();
            if let Some(end_index_relative) = executor_output[start_index..].find("</promise>") {
                let end_index = start_index + end_index_relative;
                let value = executor_output[start_index..end_index].trim().to_string();
                if Self::is_complete(&value) {
                    return Some(value);
                }
                search_start = end_index + "</promise>".len();
            } else {
                break;
            }
        }
        None
    }

    /// Persist the detected promise value to the configured promise file.
    pub fn write_promise(promise_file: &Path, value: &str) -> Result<(), AppError> {
        if let Some(parent) = promise_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(promise_file, value).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to write promise file {}: {}",
                    promise_file.display(),
                    e
                ),
            )
        })
    }

    /// Read the persisted promise entry from disk.
    pub fn read_promise(promise_file: &Path) -> Result<String, AppError> {
        if !promise_file.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(promise_file).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to read promise file {}: {}",
                    promise_file.display(),
                    e
                ),
            )
        })
    }

    /// Determine whether the provided promise value signals completion.
    pub fn is_complete(promise_value: &str) -> bool {
        !promise_value.is_empty() && promise_value.to_lowercase().contains("complete")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_promise_complete() {
        let output = "Some log <promise>COMPLETE</promise> more";
        assert_eq!(
            PromiseDetector::detect_promise(output),
            Some("COMPLETE".into())
        );
    }

    #[test]
    fn test_detect_promise_incomplete() {
        let output = "<promise>working</promise>";
        assert!(PromiseDetector::detect_promise(output).is_none());
    }

    #[test]
    fn test_write_and_read_promise() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state/promise.txt");
        PromiseDetector::write_promise(&path, "COMPLETE").unwrap();
        let stored = PromiseDetector::read_promise(&path).unwrap();
        assert_eq!(stored, "COMPLETE");
    }

    #[test]
    fn test_is_complete_case_insensitive() {
        assert!(PromiseDetector::is_complete("coMplEte"));
    }
}

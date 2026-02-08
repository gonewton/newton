#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use regex::Regex;
use std::fs;
use std::path::Path;

const PROMISE_FILE_DEFAULT: &str = ".newton/state/promise.txt";

/// Detects complete signals emitted by executor tools.
pub struct PromiseDetector;

impl PromiseDetector {
    /// Scan executor output for the first `<promise>...</promise>` whose body contains "complete" (case-insensitive).
    pub fn detect_promise(output: &str) -> Option<String> {
        let re = Regex::new(r"<promise>(?P<value>[^<]+)</promise>").unwrap();
        for cap in re.captures_iter(output) {
            let value = cap.name("value").unwrap().as_str().trim();
            if Self::is_complete(value) {
                return Some(value.to_string());
            }
        }
        None
    }

    /// Write the resolved promise value to the configured file.
    pub fn write_promise(promise_file: &Path, value: &str) -> Result<(), AppError> {
        if let Some(parent) = promise_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(promise_file, value).map_err(AppError::from)
    }

    /// Read the file containing the last detected promise.
    pub fn read_promise(promise_file: &Path) -> Result<String, AppError> {
        if !promise_file.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(promise_file).map_err(AppError::from)
    }

    /// Determine whether the provided value describes completion.
    pub fn is_complete(promise_value: &str) -> bool {
        promise_value.to_lowercase().contains("complete")
    }

    /// Default path used when no configuration overrides exist.
    pub fn default_promise_file() -> &'static str {
        PROMISE_FILE_DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detects_complete_promise_case_insensitive() {
        let text = "Some output <promise>Complete</promise> more";
        assert_eq!(
            PromiseDetector::detect_promise(text),
            Some("Complete".to_string())
        );
        let text = "<promise>complete</promise>";
        assert_eq!(
            PromiseDetector::detect_promise(text),
            Some("complete".to_string())
        );
    }

    #[test]
    fn ignores_incomplete_promise() {
        let text = "<promise>in_progress</promise>";
        assert!(PromiseDetector::detect_promise(text).is_none());
    }

    #[test]
    fn round_trip_write_and_read() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("promise.txt");
        PromiseDetector::write_promise(&path, "COMPLETE").unwrap();
        let value = PromiseDetector::read_promise(&path).unwrap();
        assert_eq!(value, "COMPLETE");
    }
}

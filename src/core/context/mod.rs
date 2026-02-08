#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use chrono::{SecondsFormat, Utc};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const CONTEXT_HEADER: &str = "# Newton Loop Context\n\n";

/// Manages the lifecycle of the Newton context board, which stores
/// markdown notes that persist for the duration of a workspace run.
pub struct ContextManager;

impl ContextManager {
    /// Add a new entry to the workspace context log.
    pub fn add_context(context_file: &Path, title: &str, message: &str) -> Result<(), AppError> {
        Self::ensure_header(context_file)?;
        Self::append_entry(context_file, title, message)
    }

    /// Append a context entry with timestamped heading and body.
    pub fn append_context(context_file: &Path, title: &str, message: &str) -> Result<(), AppError> {
        Self::ensure_header(context_file)?;
        Self::append_entry(context_file, title, message)
    }

    /// Read the current context buffer for inclusion in prompts.
    pub fn read_context(context_file: &Path) -> Result<String, AppError> {
        if !context_file.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(context_file).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to read context file {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })
    }

    /// Reset the context file to an empty header so new entries can be added.
    pub fn clear_context(context_file: &Path) -> Result<(), AppError> {
        if let Some(parent) = context_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(context_file, CONTEXT_HEADER).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to clear context file {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })
    }

    fn ensure_header(context_file: &Path) -> Result<(), AppError> {
        if context_file.exists() {
            return Ok(());
        }
        if let Some(parent) = context_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(context_file, CONTEXT_HEADER).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to init context file {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })
    }

    fn append_entry(context_file: &Path, title: &str, message: &str) -> Result<(), AppError> {
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(context_file)
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to open context file {}: {}",
                        context_file.display(),
                        e
                    ),
                )
            })?;
        writeln!(file, "## Context added at {}", timestamp).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to write context header to {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })?;
        writeln!(file, "{}\n", title).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to write context title to {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })?;
        writeln!(file, "{}\n", message).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to write context body to {}: {}",
                    context_file.display(),
                    e
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_add_and_read_context() {
        let tmp = TempDir::new().unwrap();
        let context_file = tmp.path().join("state/context.md");

        ContextManager::add_context(&context_file, "Title", "Details").unwrap();
        let contents = ContextManager::read_context(&context_file).unwrap();
        assert!(contents.contains("Title"));
        assert!(contents.contains("Details"));
    }

    #[test]
    fn test_clear_context() {
        let tmp = TempDir::new().unwrap();
        let context_file = tmp.path().join("state/context.md");

        ContextManager::add_context(&context_file, "After", "Clear").unwrap();
        ContextManager::clear_context(&context_file).unwrap();
        let contents = ContextManager::read_context(&context_file).unwrap();
        assert_eq!(contents, CONTEXT_HEADER);
    }
}

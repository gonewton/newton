#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const CONTEXT_HEADER: &str = "# Newton Loop Context\n\n";

/// Manages the Newton context file shared across iterations.
pub struct ContextManager;

impl ContextManager {
    /// Append a new context entry behind the standard header.
    pub fn add_context(context_file: &Path, title: &str, message: &str) -> Result<(), AppError> {
        Self::ensure_context_file(context_file)?;
        Self::append_context(context_file, title, message)
    }

    /// Read the entire context file, returning an empty string when it is missing.
    pub fn read_context(context_file: &Path) -> Result<String, AppError> {
        if !context_file.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(context_file).map_err(AppError::from)
    }

    /// Clear all context entries and re-write the header.
    pub fn clear_context(context_file: &Path) -> Result<(), AppError> {
        if let Some(parent) = context_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(context_file, CONTEXT_HEADER).map_err(AppError::from)
    }

    /// Append an entry to the existing context file.
    pub fn append_context(context_file: &Path, title: &str, message: &str) -> Result<(), AppError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(context_file)?;

        let timestamp = Utc::now().to_rfc3339();
        writeln!(file, "## Context added at {}", timestamp)?;
        if !title.is_empty() {
            writeln!(file, "{}", title)?;
        }
        writeln!(file)?;
        writeln!(file, "{}", message)?;
        writeln!(file)?;
        Ok(())
    }

    fn ensure_context_file(context_file: &Path) -> Result<(), AppError> {
        if let Some(parent) = context_file.parent() {
            fs::create_dir_all(parent)?;
        }
        if !context_file.exists() {
            fs::write(context_file, CONTEXT_HEADER)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn adds_and_reads_context_entries() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("context.md");

        ContextManager::add_context(&path, "Test", "Hello").unwrap();
        let content = ContextManager::read_context(&path).unwrap();
        assert!(content.contains("# Newton Loop Context"));
        assert!(content.contains("Test"));
        assert!(content.contains("Hello"));

        ContextManager::add_context(&path, "More", "World").unwrap();
        let content = ContextManager::read_context(&path).unwrap();
        assert!(content.matches("## Context added at").count() >= 2);
    }

    #[test]
    fn clears_context() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("context.md");

        ContextManager::add_context(&path, "Test", "Hello").unwrap();
        ContextManager::clear_context(&path).unwrap();
        let content = ContextManager::read_context(&path).unwrap();
        assert_eq!(content, CONTEXT_HEADER);
    }
}

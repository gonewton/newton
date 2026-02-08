#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Information about a discovered template under `.newton/templates/`.
pub struct TemplateInfo {
    /// Template name (directory name).
    pub name: String,
    /// Path to the template directory.
    pub path: PathBuf,
}

/// Represents a template that can be rendered into a workspace.
pub struct Template {
    /// Template name.
    pub name: String,
    /// Directory that contains the template artifacts.
    pub path: PathBuf,
}

/// Discovers templates that live inside `.newton/templates/` inside a workspace.
pub struct TemplateManager;

impl TemplateManager {
    /// List the templates that are currently installed in the workspace.
    pub fn list_templates(workspace_path: &Path) -> Result<Vec<TemplateInfo>, AppError> {
        let templates_dir = workspace_path.join(".newton/templates");
        if !templates_dir.exists() {
            return Ok(Vec::new());
        }

        let mut infos = Vec::new();
        for entry in fs::read_dir(&templates_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to scan templates directory {}: {}",
                    templates_dir.display(),
                    e
                ),
            )
        })? {
            let entry = entry.map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to inspect template entry in {}: {}",
                        templates_dir.display(),
                        e
                    ),
                )
            })?;

            if entry
                .file_type()
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!(
                            "Failed to read template entry metadata {}: {}",
                            entry.path().display(),
                            e
                        ),
                    )
                })?
                .is_dir()
            {
                let name = entry.file_name().to_string_lossy().to_string();
                infos.push(TemplateInfo {
                    name,
                    path: entry.path(),
                });
            }
        }

        Ok(infos)
    }

    /// Get a specific template by name.
    pub fn get_template(workspace_path: &Path, name: &str) -> Result<Template, AppError> {
        let templates = Self::list_templates(workspace_path)?;
        for info in templates {
            if info.name == name {
                return Ok(Template {
                    name: info.name,
                    path: info.path,
                });
            }
        }
        Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "Template '{}' not found under {}/.newton/templates/",
                name,
                workspace_path.display()
            ),
        ))
    }
}

/// Responsible for copying a template into the workspace and rendering variables.
pub struct TemplateRenderer;

impl TemplateRenderer {
    /// Render the named template into the workspace, substituting template variables.
    pub fn render_template(
        workspace_path: &Path,
        template_name: &str,
        variables: HashMap<String, String>,
    ) -> Result<(), AppError> {
        let template = TemplateManager::get_template(workspace_path, template_name)?;

        // Ensure the target `.newton` directory exists.
        let target_root = workspace_path.join(".newton");
        fs::create_dir_all(&target_root).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to create workspace .newton directory {}: {}",
                    target_root.display(),
                    e
                ),
            )
        })?;

        Self::render_directory(&template.path, &template.path, workspace_path, &variables)
    }

    fn render_directory(
        template_root: &Path,
        current: &Path,
        workspace_path: &Path,
        variables: &HashMap<String, String>,
    ) -> Result<(), AppError> {
        for entry in fs::read_dir(current).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "Failed to read template directory {}: {}",
                    current.display(),
                    e
                ),
            )
        })? {
            let entry = entry.map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to evaluate template entry {}: {}",
                        current.display(),
                        e
                    ),
                )
            })?;
            let path = entry.path();
            let rel_path = path
                .strip_prefix(template_root)
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!(
                            "Failed to compute relative template path {}: {}",
                            path.display(),
                            e
                        ),
                    )
                })?
                .to_path_buf();

            if path.is_dir() {
                let dir_target = workspace_path.join(".newton").join(&rel_path);
                fs::create_dir_all(&dir_target).map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!(
                            "Failed to create target directory {}: {}",
                            dir_target.display(),
                            e
                        ),
                    )
                })?;
                Self::render_directory(template_root, &path, workspace_path, variables)?;
                continue;
            }

            let target_path = if rel_path == Path::new("newton.toml") {
                workspace_path.join("newton.toml")
            } else {
                workspace_path.join(".newton").join(&rel_path)
            };

            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("Failed to create target parent {}: {}", parent.display(), e),
                    )
                })?;
            }

            let mut contents = fs::read_to_string(&path).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("Failed to read template file {}: {}", path.display(), e),
                )
            })?;

            for (key, value) in variables {
                contents = contents.replace(&format!("{{{{{}}}}}", key), value);
            }

            fs::write(&target_path, contents).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to write rendered file {}: {}",
                        target_path.display(),
                        e
                    ),
                )
            })?;

            if target_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("sh"))
                .unwrap_or(false)
            {
                Self::make_executable(&target_path)?;
            }
        }
        Ok(())
    }

    #[allow(unused_variables)]
    fn make_executable(target_path: &Path) -> Result<(), AppError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(target_path)
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!(
                            "Failed to read permissions for {}: {}",
                            target_path.display(),
                            e
                        ),
                    )
                })?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(target_path, perms).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "Failed to set executable bit on {}: {}",
                        target_path.display(),
                        e
                    ),
                )
            })?;
        }
        #[cfg(windows)]
        {
            // Windows uses file extensions to determine executability.
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn list_templates_returns_empty_when_missing() {
        let tmp = TempDir::new().unwrap();
        let templates = TemplateManager::list_templates(tmp.path()).unwrap();
        assert!(templates.is_empty());
    }

    #[test]
    fn render_template_replaces_values_and_copies_files() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();
        let template_dir = workspace.join(".newton/templates/basic");
        fs::create_dir_all(&template_dir).unwrap();
        fs::write(
            template_dir.join("executor.sh"),
            "#!/bin/bash\necho {{project_name}}\n",
        )
        .unwrap();
        fs::write(
            template_dir.join("newton.toml"),
            "[project]\nname = \"{{project_name}}\"\n",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("project_name".to_string(), "TestProj".to_string());

        TemplateRenderer::render_template(workspace, "basic", vars).unwrap();

        let executor = workspace.join(".newton/executor.sh");
        assert!(executor.exists());
        let contents = fs::read_to_string(&executor).unwrap();
        assert!(contents.contains("TestProj"));

        let toml = workspace.join("newton.toml");
        assert!(toml.exists());
        let toml_contents = fs::read_to_string(&toml).unwrap();
        assert!(toml_contents.contains("TestProj"));
    }
}

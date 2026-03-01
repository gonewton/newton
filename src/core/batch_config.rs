use crate::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Batch configuration derived from `.newton/configs/<project>.conf`.
#[derive(Debug, Clone)]
pub struct BatchProjectConfig {
    /// Absolute path to the project root that contains `.newton`.
    pub project_root: PathBuf,

    /// Required workflow file path for batch execution.
    pub workflow_file: PathBuf,
}

impl BatchProjectConfig {
    /// Load and validate batch config for the provided project ID from the workspace root.
    pub fn load(workspace_root: &Path, project_id: &str) -> Result<Self> {
        let conf_path = workspace_root
            .join(".newton")
            .join("configs")
            .join(format!("{project_id}.conf"));

        let settings = parse_conf(&conf_path)?;
        let project_root = load_and_validate_project_root(&settings, workspace_root, &conf_path)?;

        // Load workflow_file (required)
        let workflow_file_value = settings
            .get("workflow_file")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("workflow_file is required in {}", conf_path.display())
            })?;

        let workflow_file = if PathBuf::from(workflow_file_value).is_absolute() {
            PathBuf::from(workflow_file_value)
        } else {
            let project_relative = project_root.join(workflow_file_value);
            if project_relative.exists() {
                project_relative
            } else {
                workspace_root.join(workflow_file_value)
            }
        };

        Ok(BatchProjectConfig {
            project_root,
            workflow_file,
        })
    }
}

fn load_and_validate_project_root(
    settings: &HashMap<String, String>,
    workspace_root: &Path,
    conf_path: &Path,
) -> Result<PathBuf> {
    let project_root_value = settings
        .get("project_root")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("project_root is required in {}", conf_path.display()))?;

    let project_root_path = PathBuf::from(project_root_value);
    let project_root = if project_root_path.is_absolute() {
        project_root_path
    } else {
        workspace_root.join(project_root_path)
    };

    let project_newton = project_root.join(".newton");
    if !project_newton.exists() || !project_newton.is_dir() {
        return Err(anyhow::anyhow!(
            "project_root {} must contain .newton",
            project_root.display()
        ));
    }

    Ok(project_root)
}

// Old config loading functions removed - batch now workflow-only

/// Parse .conf file as key=value pairs.
pub fn parse_conf(path: &Path) -> Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read batch config {}: {}", path.display(), e))?;

    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("");
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(pos) = line.find('=') {
            let key = line[..pos].trim();
            let value = line[pos + 1..].trim();
            if key.is_empty() {
                continue;
            }
            map.insert(key.to_string(), value.to_string());
        }
    }
    Ok(map)
}

/// Find workspace root by looking for .newton directory
pub fn find_workspace_root(current_dir: &Path) -> Result<PathBuf> {
    let mut path = current_dir.to_path_buf();
    loop {
        if path.join(".newton").is_dir() {
            return Ok(path);
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            return Err(anyhow::anyhow!(
                "No .newton directory found in {} or any parent directory",
                current_dir.display()
            ));
        }
    }
}

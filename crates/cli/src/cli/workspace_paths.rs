//! Centralised Newton workspace path derivation.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

/// All standard Newton directory paths derived from a single workspace root.
pub struct WorkspacePaths {
    pub workspace_root: PathBuf,
    pub dot_newton: PathBuf,
    pub backend_sqlite: PathBuf,
    pub configs_dir: PathBuf,
    pub monitor_conf: PathBuf,
    pub plan_dir: PathBuf,
    pub workflows_state_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    // Exists flags, populated at construction time
    pub dot_newton_exists: bool,
    pub backend_sqlite_exists: bool,
    pub configs_dir_exists: bool,
    pub monitor_conf_exists: bool,
    pub plan_dir_exists: bool,
    pub workflows_state_dir_exists: bool,
    pub artifacts_dir_exists: bool,
}

impl WorkspacePaths {
    /// Build paths from an explicit workspace root.
    /// `root` need not exist; paths are computed but existence flags reflect FS state.
    pub fn new(root: PathBuf) -> Self {
        // Best-effort canonicalization; fall back to computed absolute path.
        let workspace_root = std::fs::canonicalize(&root).unwrap_or_else(|_| {
            if root.is_absolute() {
                root.clone()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(&root))
                    .unwrap_or(root.clone())
            }
        });

        let dot_newton = workspace_root.join(".newton");
        let workflows_state_dir = dot_newton.join("state");
        let backend_sqlite = workflows_state_dir.join("backend.sqlite");
        let configs_dir = dot_newton.join("configs");
        let monitor_conf = configs_dir.join("monitor.conf");
        let plan_dir = dot_newton.join("plan");
        let artifacts_dir = workflows_state_dir.join("artifacts");

        let dot_newton_exists = dot_newton.exists();
        let backend_sqlite_exists = backend_sqlite.exists();
        let configs_dir_exists = configs_dir.exists();
        let monitor_conf_exists = monitor_conf.exists();
        let plan_dir_exists = plan_dir.exists();
        let workflows_state_dir_exists = workflows_state_dir.exists();
        let artifacts_dir_exists = artifacts_dir.exists();

        Self {
            workspace_root,
            dot_newton,
            backend_sqlite,
            configs_dir,
            monitor_conf,
            plan_dir,
            workflows_state_dir,
            artifacts_dir,
            dot_newton_exists,
            backend_sqlite_exists,
            configs_dir_exists,
            monitor_conf_exists,
            plan_dir_exists,
            workflows_state_dir_exists,
            artifacts_dir_exists,
        }
    }

    /// Build paths from the process current working directory.
    /// Returns `Err` only if `current_dir()` fails (e.g. CWD was deleted).
    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|e| anyhow!("CLI-OPS-006: failed to determine current directory: {e}"))?;
        Ok(Self::new(cwd))
    }

    /// Serialize as a serde_json object with `*_exists` booleans.
    pub fn to_json_object(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert(
            "workspace_root".into(),
            json!(self.workspace_root.display().to_string()),
        );
        map.insert(
            "dot_newton".into(),
            json!(self.dot_newton.display().to_string()),
        );
        map.insert("dot_newton_exists".into(), json!(self.dot_newton_exists));
        map.insert(
            "backend_sqlite".into(),
            json!(self.backend_sqlite.display().to_string()),
        );
        map.insert(
            "backend_sqlite_exists".into(),
            json!(self.backend_sqlite_exists),
        );
        map.insert(
            "configs_dir".into(),
            json!(self.configs_dir.display().to_string()),
        );
        map.insert("configs_dir_exists".into(), json!(self.configs_dir_exists));
        map.insert(
            "monitor_conf".into(),
            json!(self.monitor_conf.display().to_string()),
        );
        map.insert(
            "monitor_conf_exists".into(),
            json!(self.monitor_conf_exists),
        );
        map.insert(
            "plan_dir".into(),
            json!(self.plan_dir.display().to_string()),
        );
        map.insert("plan_dir_exists".into(), json!(self.plan_dir_exists));
        map.insert(
            "workflows_state_dir".into(),
            json!(self.workflows_state_dir.display().to_string()),
        );
        map.insert(
            "workflows_state_dir_exists".into(),
            json!(self.workflows_state_dir_exists),
        );
        map.insert(
            "artifacts_dir".into(),
            json!(self.artifacts_dir.display().to_string()),
        );
        map.insert(
            "artifacts_dir_exists".into(),
            json!(self.artifacts_dir_exists),
        );
        map
    }

    /// Returns the SQLite connection URL for the backend store.
    /// Format: `sqlite:<absolute-path>?mode=rwc`
    pub fn backend_sqlite_url(&self) -> String {
        format!("sqlite:{}?mode=rwc", self.backend_sqlite.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_derives_standard_paths() {
        let root = PathBuf::from("/tmp/test-newton-workspace-338");
        let paths = WorkspacePaths::new(root);

        assert!(paths.workspace_root.is_absolute());
        assert!(paths.dot_newton.ends_with(".newton"));
        assert!(paths
            .backend_sqlite
            .to_string_lossy()
            .contains("backend.sqlite"));
        assert!(paths.configs_dir.to_string_lossy().contains("configs"));
        assert!(paths
            .monitor_conf
            .to_string_lossy()
            .contains("monitor.conf"));
        assert!(paths.plan_dir.to_string_lossy().contains("plan"));
        assert!(paths
            .workflows_state_dir
            .to_string_lossy()
            .contains("state"));
        assert!(paths.artifacts_dir.to_string_lossy().contains("artifacts"));
    }

    #[test]
    fn test_from_cwd_succeeds() {
        let paths = WorkspacePaths::from_cwd().expect("from_cwd should succeed");
        assert!(paths.workspace_root.is_absolute());
    }

    #[test]
    fn test_to_json_object_contains_all_keys() {
        let root = PathBuf::from("/tmp/test-newton-workspace-338");
        let paths = WorkspacePaths::new(root);
        let obj = paths.to_json_object();

        // Eight path keys
        assert!(obj.contains_key("workspace_root"));
        assert!(obj.contains_key("dot_newton"));
        assert!(obj.contains_key("backend_sqlite"));
        assert!(obj.contains_key("configs_dir"));
        assert!(obj.contains_key("monitor_conf"));
        assert!(obj.contains_key("plan_dir"));
        assert!(obj.contains_key("workflows_state_dir"));
        assert!(obj.contains_key("artifacts_dir"));

        // Seven exists keys
        assert!(obj.contains_key("dot_newton_exists"));
        assert!(obj.contains_key("backend_sqlite_exists"));
        assert!(obj.contains_key("configs_dir_exists"));
        assert!(obj.contains_key("monitor_conf_exists"));
        assert!(obj.contains_key("plan_dir_exists"));
        assert!(obj.contains_key("workflows_state_dir_exists"));
        assert!(obj.contains_key("artifacts_dir_exists"));
    }

    #[test]
    fn test_backend_sqlite_url_format() {
        let root = PathBuf::from("/tmp/test-newton-workspace-338");
        let paths = WorkspacePaths::new(root);
        let url = paths.backend_sqlite_url();
        assert!(url.starts_with("sqlite:"));
        assert!(url.ends_with("?mode=rwc"));
        assert!(url.contains("backend.sqlite"));
    }

    #[test]
    fn test_exists_flags_false_for_nonexistent_root() {
        let root = PathBuf::from("/tmp/this-path-should-not-exist-338-abc");
        let paths = WorkspacePaths::new(root);
        assert!(!paths.dot_newton_exists);
        assert!(!paths.backend_sqlite_exists);
        assert!(!paths.configs_dir_exists);
        assert!(!paths.monitor_conf_exists);
        assert!(!paths.plan_dir_exists);
        assert!(!paths.workflows_state_dir_exists);
        assert!(!paths.artifacts_dir_exists);
    }
}

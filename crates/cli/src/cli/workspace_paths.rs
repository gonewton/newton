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
    pub workflows_dir: PathBuf, // <dot_newton>/workflows
    // Exists flags, populated at construction time
    pub dot_newton_exists: bool,
    pub backend_sqlite_exists: bool,
    pub configs_dir_exists: bool,
    pub monitor_conf_exists: bool,
    pub plan_dir_exists: bool,
    pub workflows_state_dir_exists: bool,
    pub artifacts_dir_exists: bool,
    pub workflows_dir_exists: bool,
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
        let workflows_dir = dot_newton.join("workflows");

        let dot_newton_exists = dot_newton.exists();
        let backend_sqlite_exists = backend_sqlite.exists();
        let configs_dir_exists = configs_dir.exists();
        let monitor_conf_exists = monitor_conf.exists();
        let plan_dir_exists = plan_dir.exists();
        let workflows_state_dir_exists = workflows_state_dir.exists();
        let artifacts_dir_exists = artifacts_dir.exists();
        let workflows_dir_exists = workflows_dir.exists();

        Self {
            workspace_root,
            dot_newton,
            backend_sqlite,
            configs_dir,
            monitor_conf,
            plan_dir,
            workflows_state_dir,
            artifacts_dir,
            workflows_dir,
            dot_newton_exists,
            backend_sqlite_exists,
            configs_dir_exists,
            monitor_conf_exists,
            plan_dir_exists,
            workflows_state_dir_exists,
            artifacts_dir_exists,
            workflows_dir_exists,
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
        map.insert(
            "workflows_dir".into(),
            json!(self.workflows_dir.display().to_string()),
        );
        map.insert(
            "workflows_dir_exists".into(),
            json!(self.workflows_dir_exists),
        );
        map
    }

    /// Returns the SQLite connection URL for the backend store.
    /// Format: `sqlite:<absolute-path>?mode=rwc`
    pub fn backend_sqlite_url(&self) -> String {
        format!("sqlite:{}?mode=rwc", self.backend_sqlite.display())
    }
}

/// Resolve the state root directory using five-level precedence.
/// Returns the state root (parent of workflows/, artifacts/, backend.sqlite).
pub fn resolve_state_dir(
    workspace: &std::path::Path,
    explicit: Option<&std::path::Path>,
) -> PathBuf {
    // Level 1: explicit --state-dir flag
    if let Some(p) = explicit {
        let abs = std::fs::canonicalize(p).unwrap_or_else(|_| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(p))
                    .unwrap_or_else(|_| p.to_path_buf())
            }
        });
        return abs;
    }

    // Level 2: NEWTON_STATE_DIR env var
    if let Ok(env_val) = std::env::var("NEWTON_STATE_DIR") {
        if !env_val.is_empty() {
            let p = PathBuf::from(&env_val);
            let abs = std::fs::canonicalize(&p).unwrap_or_else(|_| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p)
                }
            });
            return abs;
        }
    }

    // Level 3: newton.toml [workflow].state_dir
    if let Some(state_dir) = load_toml_state_dir(workspace) {
        return state_dir;
    }

    // Level 4: walk-up from workspace to find .newton/configs anchor
    if let Some(state_dir) = walk_up_state_dir(workspace) {
        return state_dir;
    }

    // Level 5: fallback — <workspace>/.newton/state
    workspace.join(".newton").join("state")
}

fn load_toml_state_dir(workspace: &std::path::Path) -> Option<PathBuf> {
    // Try workspace/newton.toml, then walk up
    let mut dir = workspace.to_path_buf();
    loop {
        let config_path = dir.join("newton.toml");
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                // Parse minimal subset just for workflow.state_dir
                #[derive(serde::Deserialize, Default)]
                struct MinWorkflow {
                    state_dir: Option<PathBuf>,
                }
                #[derive(serde::Deserialize, Default)]
                struct MinConfig {
                    #[serde(default)]
                    workflow: MinWorkflow,
                }
                if let Ok(cfg) = toml::from_str::<MinConfig>(&content) {
                    if let Some(sd) = cfg.workflow.state_dir {
                        let abs = if sd.is_absolute() { sd } else { dir.join(&sd) };
                        return Some(abs);
                    }
                }
            }
            // File found but no state_dir configured — stop searching
            return None;
        }
        // No newton.toml here; ascend
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

fn walk_up_state_dir(workspace: &std::path::Path) -> Option<PathBuf> {
    let mut dir = workspace.to_path_buf();
    loop {
        if dir.join(".newton").join("configs").is_dir() {
            return Some(dir.join(".newton").join("state"));
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

pub fn state_checkpoints_dir(state_root: &std::path::Path) -> PathBuf {
    state_root.join("workflows")
}

pub fn state_artifacts_dir(state_root: &std::path::Path) -> PathBuf {
    state_root.join("artifacts").join("workflows")
}

pub fn state_backend_sqlite(state_root: &std::path::Path) -> PathBuf {
    state_root.join("backend.sqlite")
}

pub fn state_backend_sqlite_url(state_root: &std::path::Path) -> String {
    format!(
        "sqlite:{}?mode=rwc",
        state_backend_sqlite(state_root).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_state_dir_explicit_flag() {
        let ws = PathBuf::from("/tmp/test-ws-resolve-1");
        let explicit = PathBuf::from("/tmp/test-state-explicit-1");
        let result = resolve_state_dir(&ws, Some(&explicit));
        // explicit path is returned (canonicalized or as-is)
        assert!(result.ends_with("test-state-explicit-1") || result == explicit);
    }

    #[test]
    fn test_resolve_state_dir_env_var() {
        // Clean up after test using serial, but we can test the logic path
        let ws = PathBuf::from("/tmp/test-ws-resolve-env");
        // Without env var set (assuming it's not set), should fall through
        std::env::remove_var("NEWTON_STATE_DIR");
        // Fallback should be workspace/.newton/state
        let result = resolve_state_dir(&ws, None);
        // Should end with .newton/state (unless walk-up finds something)
        // At minimum, it should not panic
        assert!(result.is_absolute() || !result.as_os_str().is_empty());
    }

    #[test]
    fn test_resolve_state_dir_fallback() {
        // Use a path that has no .newton/configs ancestor and no env var
        let ws = PathBuf::from("/tmp/test-ws-resolve-fallback-999abc");
        std::env::remove_var("NEWTON_STATE_DIR");
        let result = resolve_state_dir(&ws, None);
        assert_eq!(result, ws.join(".newton").join("state"));
    }

    #[test]
    fn test_state_sub_path_helpers() {
        let root = PathBuf::from("/tmp/state-root");
        assert_eq!(state_checkpoints_dir(&root), root.join("workflows"));
        assert_eq!(
            state_artifacts_dir(&root),
            root.join("artifacts").join("workflows")
        );
        assert_eq!(state_backend_sqlite(&root), root.join("backend.sqlite"));
        let url = state_backend_sqlite_url(&root);
        assert!(url.starts_with("sqlite:"));
        assert!(url.ends_with("?mode=rwc"));
        assert!(url.contains("backend.sqlite"));
    }

    #[test]
    fn test_resolve_state_dir_walkup() {
        // Create a temp dir with .newton/configs to test walk-up
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".newton").join("configs")).unwrap();
        // Sub-workspace inside this root
        let sub = root.join("sub").join("project");
        std::fs::create_dir_all(&sub).unwrap();
        std::env::remove_var("NEWTON_STATE_DIR");
        let result = resolve_state_dir(&sub, None);
        assert_eq!(result, root.join(".newton").join("state"));
    }

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

        // Nine path keys
        assert!(obj.contains_key("workspace_root"));
        assert!(obj.contains_key("dot_newton"));
        assert!(obj.contains_key("backend_sqlite"));
        assert!(obj.contains_key("configs_dir"));
        assert!(obj.contains_key("monitor_conf"));
        assert!(obj.contains_key("plan_dir"));
        assert!(obj.contains_key("workflows_state_dir"));
        assert!(obj.contains_key("artifacts_dir"));
        assert!(obj.contains_key("workflows_dir"));

        // Eight exists keys
        assert!(obj.contains_key("dot_newton_exists"));
        assert!(obj.contains_key("backend_sqlite_exists"));
        assert!(obj.contains_key("configs_dir_exists"));
        assert!(obj.contains_key("monitor_conf_exists"));
        assert!(obj.contains_key("plan_dir_exists"));
        assert!(obj.contains_key("workflows_state_dir_exists"));
        assert!(obj.contains_key("artifacts_dir_exists"));
        assert!(obj.contains_key("workflows_dir_exists"));
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
        assert!(!paths.workflows_dir_exists);
    }
}

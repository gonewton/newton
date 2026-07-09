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
}

fn make_absolute(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or_else(|_| {
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p)
        }
    })
}

impl WorkspacePaths {
    /// Build paths from an explicit workspace root.
    /// `root` need not exist; paths are computed but existence flags reflect FS state.
    pub fn new(root: PathBuf) -> Self {
        let workspace_root = make_absolute(root);

        let dot_newton = workspace_root.join(".newton");
        let workflows_state_dir = dot_newton.join("state");
        let backend_sqlite = workflows_state_dir.join("backend.sqlite");
        let configs_dir = dot_newton.join("configs");
        let monitor_conf = configs_dir.join("monitor.conf");
        let plan_dir = dot_newton.join("plan");
        let artifacts_dir = workflows_state_dir.join("artifacts");
        let workflows_dir = dot_newton.join("workflows");

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
        }
    }

    pub fn dot_newton_exists(&self) -> bool {
        self.dot_newton.exists()
    }
    pub fn backend_sqlite_exists(&self) -> bool {
        self.backend_sqlite.exists()
    }
    pub fn configs_dir_exists(&self) -> bool {
        self.configs_dir.exists()
    }
    pub fn monitor_conf_exists(&self) -> bool {
        self.monitor_conf.exists()
    }
    pub fn plan_dir_exists(&self) -> bool {
        self.plan_dir.exists()
    }
    pub fn workflows_state_dir_exists(&self) -> bool {
        self.workflows_state_dir.exists()
    }
    pub fn artifacts_dir_exists(&self) -> bool {
        self.artifacts_dir.exists()
    }
    pub fn workflows_dir_exists(&self) -> bool {
        self.workflows_dir.exists()
    }

    /// Build paths from an explicit workspace root AND an explicit, already
    /// resolved state root (the output of [`resolve_state_dir`]).
    ///
    /// This is the seam that keeps `workflow run`/`serve` (which already go
    /// through `resolve_state_dir`) and secondary consumers — grading
    /// operators, `data`, `runs` — reading and writing the *same* state tree
    /// when `--state-dir` / `NEWTON_STATE_DIR` / `newton.toml` relocates it.
    /// Only the state-bearing fields (`workflows_state_dir`, `backend_sqlite`,
    /// `artifacts_dir`) are overridden; `configs_dir`, `monitor_conf`,
    /// `plan_dir`, and `workflows_dir` stay anchored to `root` because they
    /// are workspace configuration, not state.
    pub fn with_state_dir(root: PathBuf, state_root: PathBuf) -> Self {
        let mut paths = Self::new(root);
        let state_root = make_absolute(state_root);
        paths.backend_sqlite = state_backend_sqlite(&state_root);
        paths.artifacts_dir = state_root.join("artifacts");
        paths.workflows_state_dir = state_root;
        paths
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
        for (name, path) in [
            ("dot_newton", &self.dot_newton),
            ("backend_sqlite", &self.backend_sqlite),
            ("configs_dir", &self.configs_dir),
            ("monitor_conf", &self.monitor_conf),
            ("plan_dir", &self.plan_dir),
            ("workflows_state_dir", &self.workflows_state_dir),
            ("artifacts_dir", &self.artifacts_dir),
            ("workflows_dir", &self.workflows_dir),
        ] {
            map.insert(name.into(), json!(path.display().to_string()));
            map.insert(format!("{name}_exists"), json!(path.exists()));
        }
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
        return make_absolute(p.to_path_buf());
    }

    // Level 2: NEWTON_STATE_DIR env var
    if let Ok(env_val) = std::env::var("NEWTON_STATE_DIR") {
        if !env_val.is_empty() {
            return make_absolute(PathBuf::from(&env_val));
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
    #[serial_test::serial]
    fn test_resolve_state_dir_env_var() {
        let ws = PathBuf::from("/tmp/test-ws-resolve-env-set");
        let env_state = PathBuf::from("/tmp/test-state-from-env-var");
        // Set NEWTON_STATE_DIR and verify it takes precedence over fallback
        std::env::set_var("NEWTON_STATE_DIR", &env_state);
        let result = resolve_state_dir(&ws, None);
        std::env::remove_var("NEWTON_STATE_DIR");
        assert_eq!(result, env_state);
    }

    #[test]
    #[serial_test::serial]
    fn test_resolve_state_dir_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = tmp.path().to_path_buf();
        let toml_state = "/tmp/test-state-from-toml";
        std::fs::write(
            ws.join("newton.toml"),
            format!("[workflow]\nstate_dir = \"{toml_state}\"\n"),
        )
        .unwrap();
        std::env::remove_var("NEWTON_STATE_DIR");
        let result = resolve_state_dir(&ws, None);
        assert_eq!(result, PathBuf::from(toml_state));
    }

    #[test]
    #[serial_test::serial]
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
    #[serial_test::serial]
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
    fn test_with_state_dir_overrides_state_fields_only() {
        let root = PathBuf::from("/tmp/test-newton-workspace-with-state-dir");
        let state_root = PathBuf::from("/tmp/test-newton-state-override");
        let paths = WorkspacePaths::with_state_dir(root.clone(), state_root.clone());

        // State-bearing fields follow the override.
        assert_eq!(paths.workflows_state_dir, state_root);
        assert_eq!(paths.backend_sqlite, state_root.join("backend.sqlite"));
        assert_eq!(paths.artifacts_dir, state_root.join("artifacts"));
        assert!(paths
            .backend_sqlite_url()
            .contains("test-newton-state-override"));

        // Non-state fields stay anchored to the workspace root.
        let expected_root = make_absolute(root);
        assert_eq!(paths.workspace_root, expected_root);
        assert_eq!(paths.dot_newton, expected_root.join(".newton"));
        assert_eq!(
            paths.configs_dir,
            expected_root.join(".newton").join("configs")
        );
        assert_eq!(
            paths.monitor_conf,
            expected_root
                .join(".newton")
                .join("configs")
                .join("monitor.conf")
        );
        assert_eq!(paths.plan_dir, expected_root.join(".newton").join("plan"));
        assert_eq!(
            paths.workflows_dir,
            expected_root.join(".newton").join("workflows")
        );
    }

    #[test]
    fn test_exists_flags_false_for_nonexistent_root() {
        let root = PathBuf::from("/tmp/this-path-should-not-exist-338-abc");
        let paths = WorkspacePaths::new(root);
        assert!(!paths.dot_newton_exists());
        assert!(!paths.backend_sqlite_exists());
        assert!(!paths.configs_dir_exists());
        assert!(!paths.monitor_conf_exists());
        assert!(!paths.plan_dir_exists());
        assert!(!paths.workflows_state_dir_exists());
        assert!(!paths.artifacts_dir_exists());
        assert!(!paths.workflows_dir_exists());
    }
}

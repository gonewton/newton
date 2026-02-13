use crate::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Batch configuration derived from `.newton/configs/<project>.conf`.
#[derive(Debug, Clone)]
pub struct BatchProjectConfig {
    /// Absolute path to the project root that contains `.newton`.
    pub project_root: PathBuf,

    /// Coding agent override used while running the project.
    pub coding_agent: String,

    /// Coding agent model override used while running the project.
    pub coding_model: String,

    /// Evaluator tool invocation.
    pub evaluator_cmd: Option<String>,

    /// Advisor tool invocation.
    pub advisor_cmd: Option<String>,

    /// Executor tool invocation.
    pub executor_cmd: Option<String>,

    /// Coder tool invocation (used in batch mode).
    pub coder_cmd: Option<String>,

    /// Optional hook run once before executing the Newton run.
    pub pre_run_script: Option<String>,

    /// Optional hook executed after a successful plan run.
    pub post_success_script: Option<String>,

    /// Optional hook executed after a failed plan run.
    pub post_fail_script: Option<String>,

    /// Whether to resume from an existing state directory.
    pub resume: bool,

    /// Maximum iterations (limit from config).
    pub max_iterations: Option<usize>,

    /// Maximum time (seconds) (limit from config).
    pub max_time: Option<u64>,

    /// Enable verbose logging for the run.
    pub verbose: bool,

    /// Control file name stored under the state directory. Defaults to `newton_control.json` when None.
    pub control_file: Option<String>,
}

impl BatchProjectConfig {
    /// Load and validate batch config for the provided project ID from the workspace root.
    pub fn load(workspace_root: &Path, project_id: &str) -> Result<Self> {
        let configs_dir = workspace_root.join(".newton").join("configs");
        let conf_path = configs_dir.join(format!("{project_id}.conf"));

        let settings = parse_conf(&conf_path)?;

        let project_root_value = settings
            .get("project_root")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("project_root is required in {}", conf_path.display())
            })?;

        let coding_agent = settings
            .get("coding_agent")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("coding_agent is required in {}", conf_path.display()))?
            .to_string();

        let coding_model = settings
            .get("coding_model")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("coding_model is required in {}", conf_path.display()))?
            .to_string();

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

        let project_scripts = project_newton.join("scripts");
        let workspace_scripts = workspace_root.join(".newton").join("scripts");

        let post_success_script = settings
            .get("post_success_script")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let pre_run_script = settings
            .get("pre_run_script")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let post_fail_script = settings
            .get("post_fail_script")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let evaluator_cmd = resolve_tool_command(
            &settings,
            "evaluator_cmd",
            &project_root,
            project_scripts.join("evaluator.sh"),
        );
        let advisor_cmd = resolve_tool_command(
            &settings,
            "advisor_cmd",
            &project_root,
            project_scripts.join("advisor.sh"),
        );
        let executor_cmd = resolve_tool_command(
            &settings,
            "executor_cmd",
            workspace_root,
            workspace_scripts.join("executor.sh"),
        );
        let coder_cmd = resolve_tool_command(
            &settings,
            "coder_cmd",
            workspace_root,
            workspace_scripts.join("coder.sh"),
        );

        let resume = parse_flag(&settings, "resume");

        let verbose = parse_flag(&settings, "verbose");

        let max_iterations = settings
            .get("max_iterations")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<usize>()
                    .map_err(|e| anyhow::anyhow!("invalid max_iterations value '{}': {}", s, e))
            })
            .transpose()?;

        let max_time = settings
            .get("max_time")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<u64>()
                    .map_err(|e| anyhow::anyhow!("invalid max_time value '{}': {}", s, e))
            })
            .transpose()?;

        let control_file = settings
            .get("control_file")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(BatchProjectConfig {
            project_root,
            coding_agent,
            coding_model,
            evaluator_cmd,
            advisor_cmd,
            executor_cmd,
            coder_cmd,
            pre_run_script,
            post_success_script,
            post_fail_script,
            resume,
            max_iterations,
            max_time,
            verbose,
            control_file,
        })
    }
}

/// Read key=value lines from a .conf file.
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

/// Walk upwards from `start_path` until `.newton` exists, returning the workspace root.
pub fn find_workspace_root(start_path: &Path) -> Result<PathBuf> {
    let mut current = start_path.to_path_buf();
    loop {
        let candidate = current.join(".newton");
        if candidate.is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    Err(anyhow::anyhow!(
        "workspace root not found; use --workspace PATH"
    ))
}

fn resolve_tool_command(
    settings: &HashMap<String, String>,
    key: &str,
    base: &Path,
    default_cmd: PathBuf,
) -> Option<String> {
    if let Some(value) = settings.get(key) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        let candidate_path = PathBuf::from(trimmed);
        let command_path = if candidate_path.is_absolute() {
            candidate_path
        } else {
            base.join(candidate_path)
        };
        return Some(command_path.display().to_string());
    }
    Some(default_cmd.display().to_string())
}

fn parse_flag(settings: &HashMap<String, String>, key: &str) -> bool {
    settings
        .get(key)
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false)
}

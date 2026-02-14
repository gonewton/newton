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
        ParsedBatchConfig::load(workspace_root, project_id).map(|parsed| parsed.into_batch_config())
    }
}

struct ParsedBatchConfig {
    project_root: PathBuf,
    coding_agent: String,
    coding_model: String,
    evaluator_cmd: Option<String>,
    advisor_cmd: Option<String>,
    executor_cmd: Option<String>,
    coder_cmd: Option<String>,
    pre_run_script: Option<String>,
    post_success_script: Option<String>,
    post_fail_script: Option<String>,
    resume: bool,
    max_iterations: Option<usize>,
    max_time: Option<u64>,
    verbose: bool,
    control_file: Option<String>,
}

impl ParsedBatchConfig {
    fn load(workspace_root: &Path, project_id: &str) -> Result<Self> {
        let configs_dir = workspace_root.join(".newton").join("configs");
        let conf_path = configs_dir.join(format!("{project_id}.conf"));
        let content = load_raw_config(&conf_path)?;
        let settings = parse_config_content(&content);

        let project_root = resolve_project_root(workspace_root, &conf_path, &settings)?;
        let project_scripts = project_root.join(".newton").join("scripts");
        let workspace_scripts = workspace_root.join(".newton").join("scripts");

        let coding_agent = require_setting(&settings, "coding_agent", &conf_path)?;
        let coding_model = require_setting(&settings, "coding_model", &conf_path)?;

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

        let pre_run_script = optional_setting(&settings, "pre_run_script");
        let post_success_script = optional_setting(&settings, "post_success_script");
        let post_fail_script = optional_setting(&settings, "post_fail_script");

        let resume = parse_flag(&settings, "resume");
        let verbose = parse_flag(&settings, "verbose");
        let max_iterations = parse_optional_number::<usize>(&settings, "max_iterations")?;
        let max_time = parse_optional_number::<u64>(&settings, "max_time")?;
        let control_file = optional_setting(&settings, "control_file");

        Ok(ParsedBatchConfig {
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

    fn into_batch_config(self) -> BatchProjectConfig {
        BatchProjectConfig {
            project_root: self.project_root,
            coding_agent: self.coding_agent,
            coding_model: self.coding_model,
            evaluator_cmd: self.evaluator_cmd,
            advisor_cmd: self.advisor_cmd,
            executor_cmd: self.executor_cmd,
            coder_cmd: self.coder_cmd,
            pre_run_script: self.pre_run_script,
            post_success_script: self.post_success_script,
            post_fail_script: self.post_fail_script,
            resume: self.resume,
            max_iterations: self.max_iterations,
            max_time: self.max_time,
            verbose: self.verbose,
            control_file: self.control_file,
        }
    }
}

/// Read key=value lines from a .conf file.
pub fn parse_conf(path: &Path) -> Result<HashMap<String, String>> {
    let content = load_raw_config(path)?;
    Ok(parse_config_content(&content))
}

fn load_raw_config(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read batch config {}: {}", path.display(), e))
}

fn parse_config_content(content: &str) -> HashMap<String, String> {
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
    map
}

fn require_setting(
    settings: &HashMap<String, String>,
    key: &str,
    conf_path: &Path,
) -> Result<String> {
    settings
        .get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("{} is required in {}", key, conf_path.display()))
}

fn optional_setting(settings: &HashMap<String, String>, key: &str) -> Option<String> {
    settings
        .get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_optional_number<T>(settings: &HashMap<String, String>, key: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    if let Some(value) = settings
        .get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let parsed = value
            .parse::<T>()
            .map_err(|e| anyhow::anyhow!("invalid {} value '{}': {}", key, value, e))?;
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

fn resolve_project_root(
    workspace_root: &Path,
    conf_path: &Path,
    settings: &HashMap<String, String>,
) -> Result<PathBuf> {
    let project_root_value = require_setting(settings, "project_root", conf_path)?;
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

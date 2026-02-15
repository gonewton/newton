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
        let conf_path = workspace_root
            .join(".newton")
            .join("configs")
            .join(format!("{project_id}.conf"));

        let settings = parse_conf(&conf_path)?;
        let project_root = load_and_validate_project_root(&settings, workspace_root, &conf_path)?;
        let (coding_agent, coding_model) = load_required_agent_config(&settings, &conf_path)?;

        let (project_scripts, workspace_scripts) =
            get_script_directories(&project_root, workspace_root);
        let tool_commands = load_tool_commands(
            &settings,
            &project_root,
            workspace_root,
            &project_scripts,
            &workspace_scripts,
        );
        let hook_scripts = load_hook_scripts(&settings);
        let runtime_config = load_runtime_config(&settings)?;

        Ok(BatchProjectConfig {
            project_root,
            coding_agent,
            coding_model,
            evaluator_cmd: tool_commands.evaluator_cmd,
            advisor_cmd: tool_commands.advisor_cmd,
            executor_cmd: tool_commands.executor_cmd,
            coder_cmd: tool_commands.coder_cmd,
            pre_run_script: hook_scripts.pre_run_script,
            post_success_script: hook_scripts.post_success_script,
            post_fail_script: hook_scripts.post_fail_script,
            resume: runtime_config.resume,
            max_iterations: runtime_config.max_iterations,
            max_time: runtime_config.max_time,
            verbose: runtime_config.verbose,
            control_file: runtime_config.control_file,
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

fn load_required_agent_config(
    settings: &HashMap<String, String>,
    conf_path: &Path,
) -> Result<(String, String)> {
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

    Ok((coding_agent, coding_model))
}

fn get_script_directories(project_root: &Path, workspace_root: &Path) -> (PathBuf, PathBuf) {
    let project_scripts = project_root.join(".newton").join("scripts");
    let workspace_scripts = workspace_root.join(".newton").join("scripts");
    (project_scripts, workspace_scripts)
}

struct ToolCommands {
    evaluator_cmd: Option<String>,
    advisor_cmd: Option<String>,
    executor_cmd: Option<String>,
    coder_cmd: Option<String>,
}

fn load_tool_commands(
    settings: &HashMap<String, String>,
    project_root: &Path,
    workspace_root: &Path,
    project_scripts: &Path,
    workspace_scripts: &Path,
) -> ToolCommands {
    ToolCommands {
        evaluator_cmd: resolve_tool_command(
            settings,
            "evaluator_cmd",
            project_root,
            project_scripts.join("evaluator.sh"),
        ),
        advisor_cmd: resolve_tool_command(
            settings,
            "advisor_cmd",
            project_root,
            project_scripts.join("advisor.sh"),
        ),
        executor_cmd: resolve_tool_command(
            settings,
            "executor_cmd",
            workspace_root,
            workspace_scripts.join("executor.sh"),
        ),
        coder_cmd: resolve_tool_command(
            settings,
            "coder_cmd",
            workspace_root,
            workspace_scripts.join("coder.sh"),
        ),
    }
}

struct HookScripts {
    pre_run_script: Option<String>,
    post_success_script: Option<String>,
    post_fail_script: Option<String>,
}

fn load_hook_scripts(settings: &HashMap<String, String>) -> HookScripts {
    let extract_script = |key: &str| {
        settings
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };

    HookScripts {
        pre_run_script: extract_script("pre_run_script"),
        post_success_script: extract_script("post_success_script"),
        post_fail_script: extract_script("post_fail_script"),
    }
}

struct RuntimeConfig {
    resume: bool,
    verbose: bool,
    max_iterations: Option<usize>,
    max_time: Option<u64>,
    control_file: Option<String>,
}

fn load_runtime_config(settings: &HashMap<String, String>) -> Result<RuntimeConfig> {
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

    Ok(RuntimeConfig {
        resume: parse_flag(settings, "resume"),
        verbose: parse_flag(settings, "verbose"),
        max_iterations,
        max_time,
        control_file,
    })
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

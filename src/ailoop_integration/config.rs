use crate::cli::Command;
use crate::core::batch_config::parse_conf;
use crate::Result;
use anyhow::anyhow;
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

const DEFAULT_CHANNEL_SUFFIX: &str = "newton";

#[derive(Debug, Clone, PartialEq, Eq)]
/// Models how Newton should treat optional ailoop integration.
pub enum IntegrationMode {
    /// Let Newton auto-detect whether integration should be active.
    Auto,
    /// Force integration on in best-effort mode.
    Enabled,
    /// Treat any transport failure as fatal.
    FailFast,
    /// Never enable integration even when config is present.
    Disabled,
}

impl IntegrationMode {
    fn from_env(value: Option<String>) -> Self {
        match value.map(|s| s.trim().to_lowercase()).as_deref() {
            Some("0") | Some("false") | Some("off") | Some("disabled") => IntegrationMode::Disabled,
            Some("1") | Some("true") | Some("on") | Some("enabled") => IntegrationMode::Enabled,
            Some("fail-fast") | Some("failfast") | Some("fail_fast") => IntegrationMode::FailFast,
            _ => IntegrationMode::Auto,
        }
    }
}

/// Resolved ailoop settings for the current workspace and command.
#[derive(Debug, Clone)]
pub struct AiloopConfig {
    pub http_url: Url,
    pub ws_url: Url,
    pub channel: String,
    pub workspace_root: PathBuf,
    pub workspace_identifier: String,
    pub command_context: CommandContext,
    pub fail_fast: bool,
}

impl AiloopConfig {
    /// Return the stored command context for event payloads.
    pub fn command_context(&self) -> &CommandContext {
        &self.command_context
    }
}

/// Metadata describing the CLI command that kicked off the execution.
#[derive(Debug, Clone, Serialize)]
pub struct CommandContext {
    /// CLI command name (e.g., run, batch, step).
    pub name: String,
    /// Workspace root as seen when the command ran.
    pub workspace_path: String,
    /// Additional contextual metadata for the command.
    pub details: BTreeMap<String, String>,
}

impl CommandContext {
    fn from_command(command: &Command, workspace_path: &Path) -> Self {
        let mut details = BTreeMap::new();
        let name = match command {
            Command::Run(args) => {
                details.insert(
                    "max_iterations".to_string(),
                    args.max_iterations.to_string(),
                );
                details.insert("max_time".to_string(), args.max_time.to_string());
                details.insert(
                    "tool_timeout".to_string(),
                    args.tool_timeout_seconds.to_string(),
                );
                if let Some(goal) = &args.goal {
                    details.insert("goal".to_string(), goal.clone());
                }
                "run".to_string()
            }
            Command::Batch(args) => {
                details.insert("project_id".to_string(), args.project_id.clone());
                if let Some(workspace) = &args.workspace {
                    details.insert("workspace".to_string(), workspace.display().to_string());
                }
                details.insert("once".to_string(), args.once.to_string());
                details.insert("sleep".to_string(), args.sleep.to_string());
                "batch".to_string()
            }
            Command::Step(args) => {
                if let Some(exec_id) = &args.execution_id {
                    details.insert("execution_id".to_string(), exec_id.clone());
                }
                details.insert("verbose".to_string(), args.verbose.to_string());
                "step".to_string()
            }
            Command::Init(args) => {
                if let Some(path) = &args.path {
                    details.insert("init_path".to_string(), path.display().to_string());
                }
                "init".to_string()
            }
            Command::Status(args) => {
                details.insert("execution_id".to_string(), args.execution_id.clone());
                "status".to_string()
            }
            Command::Report(args) => {
                details.insert("execution_id".to_string(), args.execution_id.clone());
                details.insert("format".to_string(), format!("{:?}", args.format));
                "report".to_string()
            }
            Command::Error(args) => {
                details.insert("execution_id".to_string(), args.execution_id.clone());
                details.insert("verbose".to_string(), args.verbose.to_string());
                "error".to_string()
            }
            Command::Monitor(_) => "monitor".to_string(),
        };

        CommandContext {
            name,
            workspace_path: workspace_path.display().to_string(),
            details,
        }
    }
}

impl std::fmt::Display for CommandContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).unwrap_or_else(|_| serde_json::json!(null).to_string())
        )
    }
}

#[derive(Debug, Default)]
struct WorkspaceConfig {
    http_url: Option<String>,
    ws_url: Option<String>,
    channel: Option<String>,
    fail_fast: Option<bool>,
}

/// Resolve the optional ailoop context for a workspace+command, honoring env/config overrides.
pub fn init_config(workspace_root: &Path, command: &Command) -> Result<Option<AiloopConfig>> {
    let integration_mode = IntegrationMode::from_env(env::var("NEWTON_AILOOP_INTEGRATION").ok());
    if matches!(integration_mode, IntegrationMode::Disabled) {
        tracing::info!("ailoop integration disabled via NEWTON_AILOOP_INTEGRATION");
        return Ok(None);
    }

    let configs_dir = workspace_root.join(".newton").join("configs");
    let workspace_config = load_workspace_config(&configs_dir)?;

    let http_url = env::var("NEWTON_AILOOP_HTTP_URL")
        .ok()
        .or(workspace_config.http_url);
    let ws_url = env::var("NEWTON_AILOOP_WS_URL")
        .ok()
        .or(workspace_config.ws_url);

    if http_url.is_none() || ws_url.is_none() {
        tracing::debug!("ailoop endpoints not configured; skipping integration");
        return Ok(None);
    }

    let http_url = Url::parse(&http_url.unwrap())
        .map_err(|e| anyhow!("invalid NEWTON_AILOOP_HTTP_URL: {}", e))?;
    let ws_url =
        Url::parse(&ws_url.unwrap()).map_err(|e| anyhow!("invalid NEWTON_AILOOP_WS_URL: {}", e))?;

    let channel = env::var("NEWTON_AILOOP_CHANNEL")
        .ok()
        .or(workspace_config.channel)
        .unwrap_or_else(|| default_channel(workspace_root, command));

    let fail_fast = env::var("NEWTON_AILOOP_FAIL_FAST")
        .ok()
        .and_then(|value| parse_bool(&value))
        .or(workspace_config.fail_fast)
        .unwrap_or(matches!(integration_mode, IntegrationMode::FailFast));

    let command_context = CommandContext::from_command(command, workspace_root);
    let workspace_identifier = workspace_root.display().to_string();

    Ok(Some(AiloopConfig {
        http_url,
        ws_url,
        channel: sanitize_channel(&channel),
        workspace_root: workspace_root.to_path_buf(),
        workspace_identifier,
        command_context,
        fail_fast,
    }))
}

fn load_workspace_config(configs_dir: &Path) -> Result<WorkspaceConfig> {
    let mut config = WorkspaceConfig::default();

    if !configs_dir.is_dir() {
        return Ok(config);
    }

    let monitor_conf = configs_dir.join("monitor.conf");
    if monitor_conf.is_file() {
        if let Some(entry) = parse_config_entry(&monitor_conf)? {
            merge_workspace_config(&mut config, entry);
        }
    }

    let mut entries: Vec<_> = fs::read_dir(configs_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if entry.path() == monitor_conf {
            continue;
        }
        if let Some(entry_config) = parse_config_entry(&entry.path())? {
            merge_workspace_config(&mut config, entry_config);
            if config.http_url.is_some() && config.ws_url.is_some() {
                break;
            }
        }
    }

    Ok(config)
}

fn merge_workspace_config(acc: &mut WorkspaceConfig, next: WorkspaceConfig) {
    if acc.http_url.is_none() {
        acc.http_url = next.http_url;
    }
    if acc.ws_url.is_none() {
        acc.ws_url = next.ws_url;
    }
    if acc.channel.is_none() {
        acc.channel = next.channel;
    }
    if acc.fail_fast.is_none() {
        acc.fail_fast = next.fail_fast;
    }
}

fn parse_config_entry(path: &Path) -> Result<Option<WorkspaceConfig>> {
    let settings = parse_conf(path)?;
    let http_url = settings
        .get("ailoop_server_http_url")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let ws_url = settings
        .get("ailoop_server_ws_url")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let channel = settings
        .get("ailoop_channel")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let fail_fast = settings
        .get("ailoop_fail_fast")
        .and_then(|value| parse_bool(value));

    if http_url.is_none() && ws_url.is_none() && channel.is_none() && fail_fast.is_none() {
        return Ok(None);
    }

    Ok(Some(WorkspaceConfig {
        http_url,
        ws_url,
        channel,
        fail_fast,
    }))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn default_channel(workspace_root: &Path, command: &Command) -> String {
    let workspace_name = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(DEFAULT_CHANNEL_SUFFIX);
    let command_name = match command {
        Command::Run(_) => "run",
        Command::Batch(_) => "batch",
        Command::Step(_) => "step",
        Command::Init(_) => "init",
        Command::Status(_) => "status",
        Command::Report(_) => "report",
        Command::Error(_) => "error",
        Command::Monitor(_) => "monitor",
    };
    sanitize_channel(&format!("{}-{}", workspace_name, command_name))
}

fn sanitize_channel(value: &str) -> String {
    let filtered: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = filtered.trim_matches('-');
    if trimmed.is_empty() {
        DEFAULT_CHANNEL_SUFFIX.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{args::MonitorArgs, Command};
    use tempfile::TempDir;

    #[test]
    fn test_parse_integration_mode() {
        assert_eq!(
            IntegrationMode::from_env(Some("0".to_string())),
            IntegrationMode::Disabled
        );
        assert_eq!(
            IntegrationMode::from_env(Some("true".to_string())),
            IntegrationMode::Enabled
        );
        assert_eq!(
            IntegrationMode::from_env(Some("fail-fast".to_string())),
            IntegrationMode::FailFast
        );
        assert_eq!(IntegrationMode::from_env(None), IntegrationMode::Auto);
    }

    #[test]
    fn test_default_channel_sanitized() {
        let temp = TempDir::new().unwrap();
        let command = Command::Monitor(MonitorArgs {
            http_url: None,
            ws_url: None,
        });
        let channel = default_channel(temp.path(), &command);
        assert!(!channel.is_empty());
        assert!(channel
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
    }
}

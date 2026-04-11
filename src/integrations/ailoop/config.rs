use crate::cli::Command;
use crate::core::batch_config::parse_conf;
use crate::Result;
use anyhow::anyhow;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

/// Ailoop endpoint configuration with validated URLs.
#[derive(Debug, Clone)]
pub struct AiloopConfig {
    /// HTTP base URL for the ailoop server.
    pub http_url: Url,
    /// WebSocket URL for the ailoop server.
    pub ws_url: Url,
    /// Channel identifier for messages.
    pub channel: String,
    /// Whether ailoop integration is enabled.
    pub enabled: bool,
    /// Whether to fail fast on ailoop errors (default: false for graceful degradation).
    pub fail_fast: bool,
}

/// Runtime context for ailoop integration containing config and workspace info.
#[derive(Debug, Clone)]
pub struct AiloopContext {
    /// Ailoop configuration.
    pub config: AiloopConfig,
    /// Workspace root path.
    pub workspace_root: PathBuf,
    /// Command being executed.
    pub command_name: String,
}

impl AiloopContext {
    /// Create a new ailoop context.
    pub fn new(config: AiloopConfig, workspace_root: PathBuf, command_name: String) -> Self {
        Self {
            config,
            workspace_root,
            command_name,
        }
    }

    /// Check if ailoop integration is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the channel identifier.
    pub fn channel(&self) -> &str {
        &self.config.channel
    }

    /// Get the HTTP URL.
    pub fn http_url(&self) -> &Url {
        &self.config.http_url
    }

    /// Get the WebSocket URL.
    pub fn ws_url(&self) -> &Url {
        &self.config.ws_url
    }
}

/// Initialize ailoop context for a given command and workspace.
/// Returns None if ailoop integration is disabled or not configured.
///
/// Configuration precedence (highest to lowest):
/// 1. Environment variables (NEWTON_AILOOP_INTEGRATION, NEWTON_AILOOP_HTTP_URL, etc.)
/// 2. Workspace config files (.newton/configs/*.conf)
/// 3. Built-in defaults
pub fn init_context(workspace_root: &Path, command: &Command) -> Result<Option<AiloopContext>> {
    // Check if integration is explicitly disabled
    if let Ok(val) = env::var("NEWTON_AILOOP_INTEGRATION") {
        if val == "0" || val.to_lowercase() == "false" || val.to_lowercase() == "disabled" {
            tracing::debug!("Ailoop integration explicitly disabled via NEWTON_AILOOP_INTEGRATION");
            return Ok(None);
        }
    }

    // Only enable for run and batch commands by default
    let command_name = match command {
        Command::Run(_) => "run",
        Command::Batch(_) => "batch",
        _ => {
            tracing::debug!("Ailoop integration not applicable for this command");
            return Ok(None);
        }
    };

    // Try to load configuration
    match load_ailoop_config(workspace_root) {
        Ok(config) => {
            if !config.enabled {
                tracing::debug!("Ailoop integration disabled in configuration");
                return Ok(None);
            }
            Ok(Some(AiloopContext::new(
                config,
                workspace_root.to_path_buf(),
                command_name.to_string(),
            )))
        }
        Err(e) => {
            // If explicitly enabled via env var but config load fails, treat as error
            if env::var("NEWTON_AILOOP_INTEGRATION").is_ok() {
                Err(e)
            } else {
                // Otherwise, just disable integration silently
                tracing::debug!("Ailoop integration not configured: {}", e);
                Ok(None)
            }
        }
    }
}

/// Load ailoop configuration with precedence handling.
fn load_ailoop_config(workspace_root: &Path) -> Result<AiloopConfig> {
    // Check for explicit enable/disable
    let enabled = match env::var("NEWTON_AILOOP_INTEGRATION") {
        Ok(val) => val == "1" || val.to_lowercase() == "true" || val.to_lowercase() == "enabled",
        Err(_) => false, // Default to disabled unless explicitly enabled or configured
    };

    // Try environment variables first (highest precedence)
    let http_url_str = env::var("NEWTON_AILOOP_HTTP_URL").ok();
    let ws_url_str = env::var("NEWTON_AILOOP_WS_URL").ok();
    let channel = env::var("NEWTON_AILOOP_CHANNEL").ok();

    // If env vars provide complete config, use them
    if let (Some(http), Some(ws), Some(chan)) = (&http_url_str, &ws_url_str, &channel) {
        let http_url = validate_url(http, "NEWTON_AILOOP_HTTP_URL")?;
        let ws_url = validate_url(ws, "NEWTON_AILOOP_WS_URL")?;
        validate_channel(chan)?;

        return Ok(AiloopConfig {
            http_url,
            ws_url,
            channel: chan.clone(),
            enabled: true,
            fail_fast: env::var("NEWTON_AILOOP_FAIL_FAST")
                .ok()
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
        });
    }

    // Otherwise try to load from workspace config files
    let configs_dir = workspace_root.join(".newton").join("configs");
    if !configs_dir.is_dir() {
        if enabled {
            return Err(anyhow!(
                "Ailoop integration enabled but workspace {} does not contain .newton/configs",
                workspace_root.display()
            ));
        } else {
            return Err(anyhow!("No ailoop configuration found"));
        }
    }

    let mut config_pair = ConfigPair {
        http_url: http_url_str,
        ws_url: ws_url_str,
        channel,
    };

    // Check monitor.conf first (preferred location)
    let monitor_conf = configs_dir.join("monitor.conf");
    if monitor_conf.is_file() {
        if let Some(pair) = parse_config_file(&monitor_conf)? {
            config_pair.merge(pair);
        }
    }

    // If still incomplete, scan other config files alphabetically
    if !config_pair.is_complete() {
        let mut entries: Vec<_> = fs::read_dir(&configs_dir)?
            .filter_map(|e| e.ok())
            .filter(|entry| entry.path().is_file() && entry.path() != monitor_conf)
            .collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            if let Some(pair) = parse_config_file(&entry.path())? {
                config_pair.merge(pair);
                if config_pair.is_complete() {
                    break;
                }
            }
        }
    }

    // Validate we have complete configuration
    if !config_pair.is_complete() {
        if enabled {
            return Err(anyhow!(
                "Ailoop integration enabled but configuration incomplete. Need: ailoop_server_http_url, ailoop_server_ws_url"
            ));
        } else {
            return Err(anyhow!("Incomplete ailoop configuration"));
        }
    }

    let http_url = validate_url(
        config_pair.http_url.as_ref().unwrap(),
        "ailoop_server_http_url",
    )?;
    let ws_url = validate_url(config_pair.ws_url.as_ref().unwrap(), "ailoop_server_ws_url")?;

    // Default channel if not specified
    let channel = config_pair.channel.unwrap_or_else(|| {
        // Generate default channel from workspace name
        workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "default".to_string())
    });
    validate_channel(&channel)?;

    Ok(AiloopConfig {
        http_url,
        ws_url,
        channel,
        enabled,
        // Known limitation: the file-based configuration path always sets `fail_fast` to false,
        // so `NEWTON_AILOOP_FAIL_FAST` only applies when all endpoints plus channel come from env vars.
        fail_fast: false,
    })
}

/// Validate a URL string and return parsed URL.
fn validate_url(url_str: &str, source: &str) -> Result<Url> {
    Url::parse(url_str).map_err(|e| anyhow!("Invalid URL in {}: '{}' - {}", source, url_str, e))
}

/// Validate a channel identifier.
fn validate_channel(channel: &str) -> Result<()> {
    if channel.is_empty() {
        return Err(anyhow!("Channel identifier cannot be empty"));
    }
    if channel.len() > 256 {
        return Err(anyhow!("Channel identifier too long (max 256 chars)"));
    }
    Ok(())
}

/// Internal helper for accumulating config from multiple sources.
struct ConfigPair {
    http_url: Option<String>,
    ws_url: Option<String>,
    channel: Option<String>,
}

impl ConfigPair {
    fn merge(&mut self, other: ConfigPair) {
        if self.http_url.is_none() {
            self.http_url = other.http_url;
        }
        if self.ws_url.is_none() {
            self.ws_url = other.ws_url;
        }
        if self.channel.is_none() {
            self.channel = other.channel;
        }
    }

    fn is_complete(&self) -> bool {
        self.http_url.is_some() && self.ws_url.is_some()
    }
}

/// Parse ailoop configuration from a .conf file.
fn parse_config_file(path: &Path) -> Result<Option<ConfigPair>> {
    let settings = parse_conf(path)?;

    let http_url = settings
        .get("ailoop_server_http_url")
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty());

    let ws_url = settings
        .get("ailoop_server_ws_url")
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty());

    let channel = settings
        .get("ailoop_channel")
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty());

    if http_url.is_none() && ws_url.is_none() && channel.is_none() {
        return Ok(None);
    }

    Ok(Some(ConfigPair {
        http_url,
        ws_url,
        channel,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_config(dir: &Path, filename: &str, content: &str) -> Result<PathBuf> {
        let configs_dir = dir.join(".newton").join("configs");
        fs::create_dir_all(&configs_dir)?;
        let path = configs_dir.join(filename);
        fs::write(&path, content)?;
        Ok(path)
    }

    #[test]
    fn test_validate_url_valid() {
        let result = validate_url("http://localhost:8080", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_url_invalid() {
        let result = validate_url("not-a-url", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_channel_valid() {
        assert!(validate_channel("my-channel").is_ok());
        assert!(validate_channel("test123").is_ok());
    }

    #[test]
    fn test_validate_channel_empty() {
        assert!(validate_channel("").is_err());
    }

    #[test]
    fn test_validate_channel_too_long() {
        let long_channel = "a".repeat(257);
        assert!(validate_channel(&long_channel).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn test_load_config_from_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let workspace = temp_dir.path();

        create_test_config(
            workspace,
            "monitor.conf",
            "ailoop_server_http_url=http://localhost:8080\nailoop_server_ws_url=ws://localhost:8080\nailoop_channel=test-channel\n",
        )?;

        // Clear any env vars that might interfere
        env::remove_var("NEWTON_AILOOP_HTTP_URL");
        env::remove_var("NEWTON_AILOOP_WS_URL");
        env::remove_var("NEWTON_AILOOP_CHANNEL");

        // Set env var to enable integration
        env::set_var("NEWTON_AILOOP_INTEGRATION", "1");
        let config = load_ailoop_config(workspace)?;
        env::remove_var("NEWTON_AILOOP_INTEGRATION");

        assert_eq!(config.http_url.as_str(), "http://localhost:8080/");
        assert_eq!(config.ws_url.as_str(), "ws://localhost:8080/");
        assert_eq!(config.channel, "test-channel");

        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_env_var_precedence() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let workspace = temp_dir.path();

        create_test_config(
            workspace,
            "monitor.conf",
            "ailoop_server_http_url=http://localhost:8080\nailoop_server_ws_url=ws://localhost:8080\n",
        )?;

        // Env vars should override file config
        env::set_var("NEWTON_AILOOP_INTEGRATION", "1");
        env::set_var("NEWTON_AILOOP_HTTP_URL", "http://override:9090");
        env::set_var("NEWTON_AILOOP_WS_URL", "ws://override:9090");
        env::set_var("NEWTON_AILOOP_CHANNEL", "env-channel");

        let config = load_ailoop_config(workspace)?;

        env::remove_var("NEWTON_AILOOP_INTEGRATION");
        env::remove_var("NEWTON_AILOOP_HTTP_URL");
        env::remove_var("NEWTON_AILOOP_WS_URL");
        env::remove_var("NEWTON_AILOOP_CHANNEL");

        assert_eq!(config.http_url.as_str(), "http://override:9090/");
        assert_eq!(config.ws_url.as_str(), "ws://override:9090/");
        assert_eq!(config.channel, "env-channel");

        Ok(())
    }

    #[test]
    fn test_config_pair_merge() {
        let mut pair1 = ConfigPair {
            http_url: Some("http://first".to_string()),
            ws_url: None,
            channel: None,
        };

        let pair2 = ConfigPair {
            http_url: Some("http://second".to_string()),
            ws_url: Some("ws://second".to_string()),
            channel: Some("channel2".to_string()),
        };

        pair1.merge(pair2);

        // First value should be kept
        assert_eq!(pair1.http_url.unwrap(), "http://first");
        // Second value should fill in missing
        assert_eq!(pair1.ws_url.unwrap(), "ws://second");
        assert_eq!(pair1.channel.unwrap(), "channel2");
    }

    #[test]
    fn test_config_pair_is_complete() {
        let complete = ConfigPair {
            http_url: Some("http://test".to_string()),
            ws_url: Some("ws://test".to_string()),
            channel: Some("test".to_string()),
        };
        assert!(complete.is_complete());

        let incomplete = ConfigPair {
            http_url: Some("http://test".to_string()),
            ws_url: None,
            channel: None,
        };
        assert!(!incomplete.is_complete());
    }
}

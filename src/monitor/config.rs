use crate::core::batch_config::parse_conf;
use crate::Result;
use anyhow::anyhow;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

/// Resolved ailoop endpoints that the monitor will connect to.
#[derive(Debug, Clone)]
pub struct MonitorEndpoints {
    /// HTTP base URL for the ailoop server.
    pub http_url: Url,
    /// WebSocket URL for the ailoop server.
    pub ws_url: Url,
    /// Underlying workspace root that provided the configuration.
    pub workspace_root: PathBuf,
}

/// Optional overrides passed through `newton monitor`.
#[derive(Debug, Clone, Default)]
pub struct MonitorOverrides {
    /// Explicit HTTP URL to use instead of the config file.
    pub http_url: Option<String>,
    /// Explicit WebSocket URL to use instead of the config file.
    pub ws_url: Option<String>,
}

/// Load the HTTP/WS URLs using the workspace `.newton/configs/` layout.
pub fn load_monitor_endpoints(
    workspace_root: &Path,
    overrides: MonitorOverrides,
) -> Result<MonitorEndpoints> {
    let mut accumulator = ConfigPair::from_overrides(overrides);

    // If both endpoints are provided via CLI, skip config directory checks
    if accumulator.ready() {
        return accumulator
            .into_endpoints(workspace_root.to_path_buf())
            .map_err(|e| anyhow!("Failed to parse monitor endpoint URLs: {}", e));
    }

    // Otherwise, check for config directory
    let configs_dir = validate_configs_dir(workspace_root)?;

    load_monitor_conf(&configs_dir, &mut accumulator)?;

    if !accumulator.ready() {
        scan_config_files(&configs_dir, &mut accumulator)?;
    }

    finalize_endpoints(accumulator, workspace_root, &configs_dir)
}

fn validate_configs_dir(workspace_root: &Path) -> Result<PathBuf> {
    let configs_dir = workspace_root.join(".newton").join("configs");
    if !configs_dir.is_dir() {
        return Err(anyhow!(
            "Monitor requires .newton/configs/ directory.\n\
             Expected location: {}\n\n\
             To fix:\n  \
             - Run 'newton init' in your workspace root, or\n  \
             - Manually create the directory and add a monitor.conf file, or\n  \
             - Provide both --http-url and --ws-url on the command line",
            configs_dir.display()
        ));
    }
    Ok(configs_dir)
}

fn load_monitor_conf(configs_dir: &Path, accumulator: &mut ConfigPair) -> Result<()> {
    let monitor_conf = configs_dir.join("monitor.conf");
    if !monitor_conf.is_file() {
        return Ok(());
    }

    if let Some(pair) = parse_config_entry(&monitor_conf)? {
        accumulator.merge(pair);
    }
    Ok(())
}

fn scan_config_files(configs_dir: &Path, accumulator: &mut ConfigPair) -> Result<()> {
    let monitor_conf = configs_dir.join("monitor.conf");
    let mut entries: Vec<_> = fs::read_dir(configs_dir)?
        .filter_map(|e| e.ok())
        .filter(|entry| entry.path().is_file())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if entry.path() == monitor_conf {
            continue;
        }
        if let Some(pair) = parse_config_entry(&entry.path())? {
            accumulator.merge(pair);
            if accumulator.ready() {
                break;
            }
        }
    }
    Ok(())
}

fn finalize_endpoints(
    accumulator: ConfigPair,
    workspace_root: &Path,
    configs_dir: &Path,
) -> Result<MonitorEndpoints> {
    if !accumulator.ready() {
        let missing = accumulator.describe_missing();
        return Err(anyhow!(
            "Monitor endpoint configuration incomplete.\n\
             Missing: {}\n\n\
             Checked locations:\n  \
             - {}/monitor.conf\n  \
             - Other .conf files in {}\n\n\
             To fix:\n  \
             - Create {}/monitor.conf with both keys:\n    \
             ailoop_server_http_url = http://127.0.0.1:8081\n    \
             ailoop_server_ws_url = ws://127.0.0.1:8080\n  \
             - Or provide missing endpoint(s) via CLI:\n    \
             {}",
            missing,
            configs_dir.display(),
            configs_dir.display(),
            configs_dir.display(),
            accumulator.describe_cli_fix()
        ));
    }

    accumulator
        .into_endpoints(workspace_root.to_path_buf())
        .map_err(|e| anyhow!("Failed to parse monitor endpoint URLs: {}", e))
}

struct ConfigPair {
    http_url: Option<String>,
    ws_url: Option<String>,
}

impl ConfigPair {
    fn from_overrides(overrides: MonitorOverrides) -> Self {
        ConfigPair {
            http_url: overrides.http_url,
            ws_url: overrides.ws_url,
        }
    }

    fn merge(&mut self, other: ConfigPair) {
        if self.http_url.is_none() {
            self.http_url = other.http_url;
        }
        if self.ws_url.is_none() {
            self.ws_url = other.ws_url;
        }
    }

    fn ready(&self) -> bool {
        self.http_url.is_some() && self.ws_url.is_some()
    }

    fn describe_missing(&self) -> String {
        match (self.http_url.is_some(), self.ws_url.is_some()) {
            (false, false) => "HTTP and WebSocket endpoints".to_string(),
            (false, true) => "HTTP endpoint (ailoop_server_http_url)".to_string(),
            (true, false) => "WebSocket endpoint (ailoop_server_ws_url)".to_string(),
            (true, true) => "none (both endpoints present)".to_string(),
        }
    }

    fn describe_cli_fix(&self) -> String {
        match (self.http_url.is_some(), self.ws_url.is_some()) {
            (false, false) => {
                "newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080"
                    .to_string()
            }
            (false, true) => "newton monitor --http-url http://127.0.0.1:8081".to_string(),
            (true, false) => "newton monitor --ws-url ws://127.0.0.1:8080".to_string(),
            (true, true) => "No CLI fix needed (both endpoints present)".to_string(),
        }
    }

    fn into_endpoints(self, workspace_root: PathBuf) -> Result<MonitorEndpoints> {
        let http_url = Url::parse(
            self.http_url
                .as_ref()
                .ok_or_else(|| anyhow!("missing HTTP endpoint"))?,
        )
        .map_err(|e| anyhow!("Invalid HTTP URL: {}", e))?;
        let ws_url = Url::parse(
            self.ws_url
                .as_ref()
                .ok_or_else(|| anyhow!("missing WebSocket endpoint"))?,
        )
        .map_err(|e| anyhow!("Invalid WebSocket URL: {}", e))?;
        Ok(MonitorEndpoints {
            http_url,
            ws_url,
            workspace_root,
        })
    }
}

fn parse_config_entry(path: &Path) -> Result<Option<ConfigPair>> {
    let settings = parse_conf(path)?;

    let http_url = settings.get("ailoop_server_http_url").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let ws_url = settings.get("ailoop_server_ws_url").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    // Return None only if both are missing; partial configs are valid
    if http_url.is_none() && ws_url.is_none() {
        return Ok(None);
    }

    Ok(Some(ConfigPair { http_url, ws_url }))
}

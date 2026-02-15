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
    let configs_dir = validate_configs_dir(workspace_root)?;
    let mut accumulator = ConfigPair::from_overrides(overrides);

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
            "Workspace {} must contain .newton/configs",
            workspace_root.display()
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
        return Err(anyhow!(
            "Could not find a config under {} that defines both ailoop_server_http_url and ailoop_server_ws_url",
            configs_dir.display()
        ));
    }

    accumulator
        .into_endpoints(workspace_root.to_path_buf())
        .map_err(|e| anyhow!("parsing monitor endpoints: {}", e))
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

    fn into_endpoints(self, workspace_root: PathBuf) -> Result<MonitorEndpoints> {
        let http_url = Url::parse(
            self.http_url
                .as_ref()
                .ok_or_else(|| anyhow!("missing HTTP URL"))?,
        )?;
        let ws_url = Url::parse(
            self.ws_url
                .as_ref()
                .ok_or_else(|| anyhow!("missing WebSocket URL"))?,
        )?;
        Ok(MonitorEndpoints {
            http_url,
            ws_url,
            workspace_root,
        })
    }
}

fn parse_config_entry(path: &Path) -> Result<Option<ConfigPair>> {
    let settings = parse_conf(path)?;

    let http_url = settings
        .get("ailoop_server_http_url")
        .map(|v| v.trim().to_string());
    let ws_url = settings
        .get("ailoop_server_ws_url")
        .map(|v| v.trim().to_string());

    if http_url.as_ref().map(|s| s.is_empty()).unwrap_or(true)
        || ws_url.as_ref().map(|s| s.is_empty()).unwrap_or(true)
    {
        return Ok(None);
    }

    Ok(Some(ConfigPair { http_url, ws_url }))
}

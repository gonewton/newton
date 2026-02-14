use crate::logging::layers::console::ConsoleOutput;
use crate::Result;
use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing_subscriber::filter::Directive;
use url::Url;

const DEFAULT_LEVEL: &str = "info";
const DEFAULT_SERVICE_NAME: &str = env!("CARGO_PKG_NAME");

/// Resolved logging configuration after reading config files and env overrides.
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub log_dir: Option<PathBuf>,
    pub default_level: String,
    pub enable_file: bool,
    pub console_output: Option<ConsoleOutput>,
    pub opentelemetry: OpenTelemetryConfig,
}

/// OpenTelemetry configuration applied when an endpoint is provided.
#[derive(Debug, Clone)]
pub struct OpenTelemetryConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub service_name: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_dir: None,
            default_level: DEFAULT_LEVEL.to_string(),
            enable_file: true,
            console_output: None,
            opentelemetry: OpenTelemetryConfig::default(),
        }
    }
}

impl Default for OpenTelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            service_name: DEFAULT_SERVICE_NAME.to_string(),
        }
    }
}

impl LoggingConfig {
    /// Load configuration with deterministic precedence: defaults, config file, env overrides.
    pub fn load(workspace_root: Option<&Path>) -> Result<Self> {
        let mut config = LoggingConfig::default();
        if let Some(workspace) = workspace_root {
            if let Some(workspace_config) = Self::load_from_workspace(workspace)? {
                config.apply(workspace_config)?;
            }
        }
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    fn load_from_workspace(workspace_root: &Path) -> Result<Option<TomlLogging>> {
        let path = workspace_root
            .join(".newton")
            .join("config")
            .join("logging.toml");
        Self::load_from_file(&path)
    }

    fn load_from_file(path: &Path) -> Result<Option<TomlLogging>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read logging config {}", path.display()))?;
        let parsed: TomlLogging = toml::from_str(&content)
            .with_context(|| format!("failed to parse logging config {}", path.display()))?;
        Ok(Some(parsed))
    }

    fn apply(&mut self, toml: TomlLogging) -> Result<()> {
        if let Some(logging) = toml.logging {
            if let Some(log_dir) = logging.log_dir {
                self.log_dir = Some(PathBuf::from(log_dir));
            }
            if let Some(default_level) = logging.default_level {
                self.default_level = default_level;
            }
            if let Some(enable_file) = logging.enable_file {
                self.enable_file = enable_file;
            }
            if let Some(console_output) = logging.console_output {
                self.console_output = Some(console_output);
            }
            if let Some(opentelemetry) = logging.opentelemetry {
                self.opentelemetry.apply(opentelemetry);
            }
        }
        Ok(())
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(endpoint) = env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            if !endpoint.trim().is_empty() {
                self.opentelemetry.endpoint = Some(endpoint);
                self.opentelemetry.enabled = true;
            }
        }
    }

    fn validate(&self) -> Result<()> {
        Directive::from_str(&self.default_level)
            .map_err(|_| anyhow!("logging.default_level must be a valid tracing directive"))?;

        if let Some(endpoint) = &self.opentelemetry.endpoint {
            let parsed = Url::parse(endpoint)
                .map_err(|err| anyhow!("invalid logging.opentelemetry.endpoint: {}", err))?;
            if parsed.scheme().is_empty() {
                return Err(anyhow!(
                    "logging.opentelemetry.endpoint must include a scheme"
                ));
            }
        }

        if self.opentelemetry.enabled && self.opentelemetry.endpoint.is_none() {
            return Err(anyhow!(
                "logging.opentelemetry.endpoint is required when opentelemetry is enabled"
            ));
        }

        if self.opentelemetry.enabled && self.opentelemetry.service_name.trim().is_empty() {
            return Err(anyhow!(
                "logging.opentelemetry.service_name must be set when opentelemetry is enabled"
            ));
        }

        Ok(())
    }
}

impl OpenTelemetryConfig {
    fn apply(&mut self, raw: TomlOpentelemetry) {
        if let Some(enabled) = raw.enabled {
            self.enabled = enabled;
        }
        if let Some(endpoint) = raw.endpoint {
            self.endpoint = Some(endpoint);
        }
        if let Some(service_name) = raw.service_name {
            self.service_name = service_name;
        }
    }
}

#[derive(Debug, Deserialize)]
struct TomlLogging {
    pub logging: Option<TomlLoggingSection>,
}

#[derive(Debug, Deserialize)]
struct TomlLoggingSection {
    pub log_dir: Option<String>,
    pub default_level: Option<String>,
    pub enable_file: Option<bool>,
    #[serde(default)]
    pub console_output: Option<ConsoleOutput>,
    pub opentelemetry: Option<TomlOpentelemetry>,
}

#[derive(Debug, Deserialize)]
struct TomlOpentelemetry {
    pub enabled: Option<bool>,
    pub endpoint: Option<String>,
    pub service_name: Option<String>,
}

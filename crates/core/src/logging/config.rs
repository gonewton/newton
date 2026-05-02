use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Console output targets supported by the logging configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleOutput {
    /// Stream logs to stdout.
    Stdout,
    /// Stream logs to stderr.
    #[default]
    Stderr,
    /// Disable console logging entirely.
    None,
}

/// OpenTelemetry settings from `.newton/config/logging.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTelemetryConfig {
    /// Whether the OpenTelemetry pipeline should be enabled.
    pub enabled: Option<bool>,
    /// OTLP endpoint used to export spans.
    pub endpoint: Option<String>,
    /// Service name override reported to the collector.
    pub service_name: Option<String>,
}

/// Parsed logging configuration that mirrors the supported TOML keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfigFile {
    /// Custom log directory from `logging.log_dir`.
    pub log_dir: Option<PathBuf>,
    /// Default log level from `logging.default_level`.
    pub default_level: Option<String>,
    /// Whether file logging is enabled via `logging.enable_file`.
    pub enable_file: Option<bool>,
    /// Target for console logging from `logging.console_output`.
    pub console_output: Option<ConsoleOutput>,
    /// Optional OpenTelemetry configuration.
    pub opentelemetry: Option<OpenTelemetryConfig>,
}

impl LoggingConfigFile {
    fn from_table(table: LoggingTable) -> Self {
        let opentelemetry = table.opentelemetry.map(|ot| OpenTelemetryConfig {
            enabled: ot.enabled,
            endpoint: ot.endpoint,
            service_name: ot.service_name,
        });
        let log_dir = table.log_dir.map(PathBuf::from);
        LoggingConfigFile {
            log_dir,
            default_level: table.default_level,
            enable_file: table.enable_file,
            console_output: table.console_output,
            opentelemetry,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoggingToml {
    logging: Option<LoggingTable>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct LoggingTable {
    log_dir: Option<String>,
    default_level: Option<String>,
    enable_file: Option<bool>,
    console_output: Option<ConsoleOutput>,
    opentelemetry: Option<RawOpenTelemetry>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawOpenTelemetry {
    enabled: Option<bool>,
    endpoint: Option<String>,
    service_name: Option<String>,
}

/// Loads `.newton/config/logging.toml`, returning `Ok(None)` when the file is absent.
pub fn load_logging_config(path: &Path) -> Result<Option<LoggingConfigFile>> {
    let content = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow!(
                "Failed to read logging config {}: {}",
                path.display(),
                err
            ))
        }
    };

    let parsed: LoggingToml = toml::from_str(&content)
        .with_context(|| format!("Failed to parse logging config {}", path.display()))?;

    Ok(parsed.logging.map(LoggingConfigFile::from_table))
}

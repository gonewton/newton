pub mod config;
pub mod context;
pub mod layers;

pub use context::{detect_context, ExecutionContext};

use crate::logging::config::{load_logging_config, ConsoleOutput, LoggingConfigFile};
use crate::logging::layers as layers_mod;
use crate::logging::layers::{console, file, opentelemetry};
use crate::{cli::Command, core::find_workspace_root, Result};
use anyhow::{anyhow, Context};
use dirs_next::home_dir;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::Layered, prelude::*, registry::Registry, EnvFilter};
use url::Url;

const DEFAULT_LOG_LEVEL: &str = "info";
const LOG_FILE_NAME: &str = "newton.log";
const CONFIG_RELATIVE_PATH: &str = ".newton/config/logging.toml";
static LOGGING_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Guard that keeps non-blocking writer guards alive for the duration of the command execution.
pub struct LoggingGuard {
    _file_guard: Option<WorkerGuard>,
    _otel_guard: Option<layers::opentelemetry::OpenTelemetryGuard>,
}

impl LoggingGuard {
    fn new(
        file_guard: Option<WorkerGuard>,
        otel_guard: Option<layers::opentelemetry::OpenTelemetryGuard>,
    ) -> Self {
        Self {
            _file_guard: file_guard,
            _otel_guard: otel_guard,
        }
    }
}

/// Initialize the reusable logging framework for the given CLI command.
///
/// This registers tracing subscribers, creates log directories, and keeps optional OpenTelemetry resources alive.
pub fn init(command: &Command) -> Result<LoggingGuard> {
    if LOGGING_INITIALIZED.load(Ordering::SeqCst) {
        return Err(anyhow!("logging already initialized"));
    }

    let context = detect_context(command);
    let workspace_root = workspace_root_for_command(command)?;

    let config = workspace_root
        .as_ref()
        .map(|root| load_logging_config(&root.join(CONFIG_RELATIVE_PATH)))
        .transpose()?
        .flatten();

    let settings = build_effective_settings(context, workspace_root.as_deref(), config.as_ref())?;

    let filter = EnvFilter::try_new(&settings.log_level)
        .with_context(|| format!("failed to create log filter from '{}'", settings.log_level))?;

    let subscriber = Registry::default();

    let mut file_guard = None;
    let file_layer = if settings.file_enabled {
        fs::create_dir_all(&settings.log_dir).with_context(|| {
            format!(
                "failed to create log directory {}",
                settings.log_dir.display()
            )
        })?;
        let (layer, guard) = file::build_file_layer::<Registry>(&settings.log_file)?;
        file_guard = Some(guard);
        layer
    } else {
        layers_mod::noop_layer::<Registry>()
    };
    type AfterFile = Layered<layers_mod::BoxLayer<Registry>, Registry>;
    let subscriber = file_layer.with_subscriber(subscriber);

    let console_layer =
        if let Some(layer) = console::build_console_layer::<AfterFile>(settings.console_output) {
            layer
        } else {
            layers_mod::noop_layer::<AfterFile>()
        };
    type AfterConsole = Layered<layers_mod::BoxLayer<AfterFile>, AfterFile>;
    let subscriber = console_layer.with_subscriber(subscriber);

    let mut otel_guard = None;
    let otel_layer = if settings.otel_decision.enabled {
        if let Some(endpoint) = &settings.otel_decision.endpoint {
            match opentelemetry::build_opentelemetry_layer::<AfterConsole>(
                endpoint,
                settings.otel_decision.service_name.as_deref(),
            ) {
                Ok((layer, guard)) => {
                    otel_guard = Some(guard);
                    layer
                }
                Err(err) => {
                    tracing::warn!("OpenTelemetry exporter disabled: {}", err);
                    layers_mod::noop_layer::<AfterConsole>()
                }
            }
        } else {
            layers_mod::noop_layer::<AfterConsole>()
        }
    } else {
        layers_mod::noop_layer::<AfterConsole>()
    };
    let subscriber = otel_layer.with_subscriber(subscriber);

    let subscriber = subscriber.with(filter);

    tracing::subscriber::set_global_default(subscriber)
        .context("failed to install tracing subscriber; check logging configuration")?;

    if let Some(warning) = &settings.otel_decision.warning {
        tracing::warn!("{}", warning);
    }

    LOGGING_INITIALIZED.store(true, Ordering::SeqCst);

    Ok(LoggingGuard::new(file_guard, otel_guard))
}

#[derive(Debug)]
pub(crate) struct EffectiveLoggingSettings {
    pub log_dir: PathBuf,
    pub log_file: PathBuf,
    pub log_level: String,
    pub file_enabled: bool,
    pub console_output: ConsoleOutput,
    pub otel_decision: OtelDecision,
}

pub(crate) fn build_effective_settings(
    context: ExecutionContext,
    workspace: Option<&Path>,
    config: Option<&LoggingConfigFile>,
) -> Result<EffectiveLoggingSettings> {
    let log_dir = determine_log_dir(workspace, config)?;
    let log_file = log_dir.join(LOG_FILE_NAME);
    let log_level = select_log_level(config);
    let file_enabled = select_file_enabled(context, config);
    let console_output = select_console_output(context, config);
    let otel_decision = determine_opentelemetry(config)?;

    Ok(EffectiveLoggingSettings {
        log_dir,
        log_file,
        log_level,
        file_enabled,
        console_output,
        otel_decision,
    })
}

fn workspace_root_for_command(command: &Command) -> Result<Option<PathBuf>> {
    let candidate = match command {
        Command::Run(args) => Some(args.path.clone()),
        Command::Step(args) => Some(args.path.clone()),
        Command::Status(args) => Some(args.path.clone()),
        Command::Report(args) => Some(args.path.clone()),
        Command::Error(_) => env::current_dir()
            .ok()
            .and_then(|cwd| find_workspace_root(&cwd).ok()),
        Command::Init(args) => args.path.clone().or_else(|| env::current_dir().ok()),
        Command::Batch(args) => {
            if let Some(ws) = &args.workspace {
                Some(ws.clone())
            } else if let Ok(cwd) = env::current_dir() {
                find_workspace_root(&cwd).ok()
            } else {
                None
            }
        }
        Command::Monitor(_) => {
            let cwd = env::current_dir()?;
            Some(find_workspace_root(&cwd)?)
        }
    };

    if let Some(mut path) = candidate {
        if let Ok(canonical) = fs::canonicalize(&path) {
            path = canonical;
        }
        if path.join(".newton").is_dir() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

fn determine_log_dir(
    workspace: Option<&Path>,
    config: Option<&LoggingConfigFile>,
) -> Result<PathBuf> {
    let newton_root = match workspace {
        Some(root) => root.join(".newton"),
        None => home_dir()
            .ok_or_else(|| anyhow!("home directory not configured; cannot resolve log path"))?
            .join(".newton"),
    };

    if let Some(cfg) = config {
        if let Some(ref custom_dir) = cfg.log_dir {
            return Ok(normalize_path(&newton_root, custom_dir));
        }
    }

    Ok(newton_root.join("logs"))
}

fn select_log_level(config: Option<&LoggingConfigFile>) -> String {
    env::var("RUST_LOG")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .or_else(|| {
            config
                .and_then(|cfg| cfg.default_level.as_deref())
                .map(str::trim)
                .filter(|trimmed| !trimmed.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string())
}

fn select_file_enabled(context: ExecutionContext, config: Option<&LoggingConfigFile>) -> bool {
    let configured = config.and_then(|cfg| cfg.enable_file).unwrap_or(true);
    if matches!(context, ExecutionContext::RemoteAgent) {
        true
    } else {
        configured
    }
}

fn select_console_output(
    context: ExecutionContext,
    config: Option<&LoggingConfigFile>,
) -> ConsoleOutput {
    if matches!(context, ExecutionContext::Tui) {
        return ConsoleOutput::None;
    }

    config
        .and_then(|cfg| cfg.console_output)
        .unwrap_or(match context {
            ExecutionContext::LocalDev => ConsoleOutput::Stderr,
            _ => ConsoleOutput::None,
        })
}

#[derive(Debug)]
pub(crate) struct OtelDecision {
    endpoint: Option<Url>,
    enabled: bool,
    service_name: Option<String>,
    warning: Option<String>,
}

fn determine_opentelemetry(config: Option<&LoggingConfigFile>) -> Result<OtelDecision> {
    let otel_config = config.and_then(|cfg| cfg.opentelemetry.as_ref());
    let enabled_flag = otel_config.and_then(|ot| ot.enabled).unwrap_or(true);
    let service_name = otel_config.and_then(|ot| ot.service_name.clone());

    if let Ok(env_value) = env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let trimmed = env_value.trim();
        if trimmed.is_empty() {
            return Ok(OtelDecision {
                endpoint: None,
                enabled: false,
                service_name,
                warning: None,
            });
        }
        match Url::parse(trimmed) {
            Ok(endpoint) => Ok(OtelDecision {
                endpoint: Some(endpoint),
                enabled: enabled_flag,
                service_name,
                warning: None,
            }),
            Err(err) => Ok(OtelDecision {
                endpoint: None,
                enabled: false,
                service_name,
                warning: Some(format!(
                    "invalid OTEL_EXPORTER_OTLP_ENDPOINT ({}); OpenTelemetry disabled",
                    err
                )),
            }),
        }
    } else if let Some(cfg_endpoint) = otel_config.and_then(|ot| ot.endpoint.clone()) {
        let trimmed = cfg_endpoint.trim();
        if trimmed.is_empty() {
            return Ok(OtelDecision {
                endpoint: None,
                enabled: false,
                service_name,
                warning: None,
            });
        }
        let endpoint = Url::parse(trimmed)
            .map_err(|err| anyhow!("invalid logging.opentelemetry.endpoint: {}", err))?;
        Ok(OtelDecision {
            endpoint: Some(endpoint),
            enabled: enabled_flag,
            service_name,
            warning: None,
        })
    } else {
        Ok(OtelDecision {
            endpoint: None,
            enabled: false,
            service_name,
            warning: None,
        })
    }
}

fn normalize_path(base: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        clean_absolute(candidate)
    } else {
        clean_relative_within(base, candidate)
    }
}

fn clean_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn clean_relative_within(base: &Path, path: &Path) -> PathBuf {
    let mut normalized = base.to_path_buf();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{
        BatchArgs, ErrorArgs, InitArgs, MonitorArgs, ReportArgs, ReportFormat, RunArgs, StatusArgs,
        StepArgs,
    };
    use crate::logging::config::OpenTelemetryConfig;
    use serial_test::serial;
    use std::env;
    use std::path::PathBuf;

    fn make_run_command() -> Command {
        Command::Run(RunArgs::for_batch(PathBuf::from("."), None))
    }

    fn make_batch_command() -> Command {
        Command::Batch(BatchArgs {
            project_id: "project".into(),
            workspace: Some(PathBuf::from(".")),
            once: false,
            sleep: 60,
        })
    }

    fn make_step_command() -> Command {
        Command::Step(StepArgs {
            path: PathBuf::from("."),
            execution_id: None,
            verbose: false,
        })
    }

    fn make_status_command() -> Command {
        Command::Status(StatusArgs {
            execution_id: "id".into(),
            path: PathBuf::from("."),
        })
    }

    fn make_report_command() -> Command {
        Command::Report(ReportArgs {
            execution_id: "id".into(),
            path: PathBuf::from("."),
            format: ReportFormat::Text,
        })
    }

    fn make_error_command() -> Command {
        Command::Error(ErrorArgs {
            execution_id: "id".into(),
            verbose: false,
        })
    }

    fn make_init_command() -> Command {
        Command::Init(InitArgs {
            path: Some(PathBuf::from(".")),
            template_source: None,
        })
    }

    fn make_monitor_command() -> Command {
        Command::Monitor(MonitorArgs {
            http_url: None,
            ws_url: None,
        })
    }

    #[test]
    #[serial]
    fn detect_context_matches_table() {
        env::remove_var("NEWTON_REMOTE_AGENT");
        let mapping = vec![
            (make_run_command(), ExecutionContext::LocalDev),
            (make_step_command(), ExecutionContext::LocalDev),
            (make_status_command(), ExecutionContext::LocalDev),
            (make_report_command(), ExecutionContext::LocalDev),
            (make_error_command(), ExecutionContext::LocalDev),
            (make_init_command(), ExecutionContext::LocalDev),
            (make_batch_command(), ExecutionContext::Batch),
            (make_monitor_command(), ExecutionContext::Tui),
        ];

        for (command, expected) in mapping {
            assert_eq!(detect_context(&command), expected);
        }
    }

    #[test]
    #[serial]
    fn detect_context_remote_override() {
        env::set_var("NEWTON_REMOTE_AGENT", "1");
        assert_eq!(
            detect_context(&make_run_command()),
            ExecutionContext::RemoteAgent
        );
        assert_eq!(
            detect_context(&make_batch_command()),
            ExecutionContext::RemoteAgent
        );
        assert_eq!(
            detect_context(&make_error_command()),
            ExecutionContext::RemoteAgent
        );
        assert_eq!(
            detect_context(&make_monitor_command()),
            ExecutionContext::Tui
        );
        env::remove_var("NEWTON_REMOTE_AGENT");
    }

    #[test]
    #[serial]
    fn select_log_level_prefers_env_then_config_then_default() {
        env::remove_var("RUST_LOG");
        let settings = LoggingConfigFile {
            log_dir: None,
            default_level: Some("warn".into()),
            enable_file: None,
            console_output: None,
            opentelemetry: None,
        };
        assert_eq!(select_log_level(Some(&settings)), "warn");
        env::set_var("RUST_LOG", "debug");
        assert_eq!(select_log_level(Some(&settings)), "debug");
        env::remove_var("RUST_LOG");
    }

    #[test]
    #[serial]
    fn determine_log_dir_prefers_workspace() {
        let original = env::var_os("HOME");
        env::remove_var("HOME");
        let workspace = PathBuf::from("/tmp/workspace");
        let dir = determine_log_dir(Some(&workspace), None).unwrap();
        assert!(dir.ends_with("workspace/.newton/logs"));
        if let Some(val) = original {
            env::set_var("HOME", val);
        }
    }

    #[test]
    #[serial]
    fn determine_log_dir_falls_back_home() {
        let tmp = tempfile::tempdir().unwrap();
        let original = env::var_os("HOME");
        env::set_var("HOME", tmp.path());
        let dir = determine_log_dir(None, None).unwrap();
        assert!(dir.starts_with(tmp.path()));
        if let Some(val) = original {
            env::set_var("HOME", val);
        } else {
            env::remove_var("HOME");
        }
    }

    #[test]
    fn select_console_output_variation() {
        assert_eq!(
            select_console_output(ExecutionContext::Tui, None),
            ConsoleOutput::None
        );
        assert_eq!(
            select_console_output(ExecutionContext::Batch, None),
            ConsoleOutput::None
        );
        assert_eq!(
            select_console_output(ExecutionContext::RemoteAgent, None),
            ConsoleOutput::None
        );
        assert_eq!(
            select_console_output(ExecutionContext::LocalDev, None),
            ConsoleOutput::Stderr
        );
    }

    #[test]
    fn select_file_enabled_respects_remote_guardrail() {
        let config = LoggingConfigFile {
            log_dir: None,
            default_level: None,
            enable_file: Some(false),
            console_output: None,
            opentelemetry: None,
        };
        assert!(!select_file_enabled(
            ExecutionContext::LocalDev,
            Some(&config)
        ));
        assert!(select_file_enabled(
            ExecutionContext::RemoteAgent,
            Some(&config)
        ));
    }

    #[test]
    #[serial]
    fn determine_opentelemetry_env_overrides_invalid() {
        env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4317");
        let decision = determine_opentelemetry(None).unwrap();
        assert!(decision.enabled);
        assert!(decision.endpoint.is_some());
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "not a url");
        let failure = determine_opentelemetry(None).unwrap();
        assert!(!failure.enabled);
        assert!(failure.warning.as_ref().unwrap().contains("invalid"));
        env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }

    #[test]
    #[serial]
    fn determine_opentelemetry_from_config() {
        env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        let mut config = LoggingConfigFile {
            log_dir: None,
            default_level: None,
            enable_file: None,
            console_output: None,
            opentelemetry: Some(OpenTelemetryConfig {
                enabled: Some(true),
                endpoint: Some("https://example.com".to_string()),
                service_name: Some("custom".to_string()),
            }),
        };
        let decision = determine_opentelemetry(Some(&config)).unwrap();
        assert!(decision.enabled);
        assert_eq!(decision.service_name.as_deref(), Some("custom"));

        config.opentelemetry = Some(OpenTelemetryConfig {
            enabled: Some(true),
            endpoint: Some("bad url".to_string()),
            service_name: None,
        });
        assert!(determine_opentelemetry(Some(&config)).is_err());
    }

    #[test]
    fn normalize_path_blocks_outside() {
        let base = PathBuf::from("/tmp/base");
        let candidate = PathBuf::from("../evil");
        let normalized = normalize_path(&base, &candidate);
        assert_eq!(normalized, PathBuf::from("/tmp/evil"));
    }
}

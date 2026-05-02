use anyhow::{anyhow, bail, Context, Result};
use newton_core::logging;
use newton_core::logging::{LogInvocation, LogInvocationKind};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let config = Config::parse()?;
    if let Some(workspace) = &config.workspace {
        env::set_current_dir(workspace)
            .context("failed to set current working directory for helper")?;
    }
    let message = config.message.clone();
    let log_dir_override = config.log_dir.clone();
    let invocation = config.into_invocation()?;

    let _guard = logging::init(&invocation, log_dir_override.as_deref())
        .context("failed to initialize logging from helper binary")?;

    tracing::info!("{}", message);

    Ok(())
}

#[derive(Debug)]
enum Mode {
    Monitor,
    LocalDev,
    Batch,
}

impl Mode {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "monitor" => Some(Mode::Monitor),
            "localdev" => Some(Mode::LocalDev),
            "batch" => Some(Mode::Batch),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Mode::Monitor => "monitor",
            Mode::LocalDev => "localdev",
            Mode::Batch => "batch",
        }
    }
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    workspace: Option<PathBuf>,
    message: String,
    log_dir: Option<PathBuf>,
}

impl Config {
    fn parse() -> Result<Self> {
        let mut args = env::args().skip(1);
        let mut mode = None;
        let mut workspace = None;
        let mut message = None;
        let mut log_dir = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "monitor" | "localdev" | "batch" => {
                    if mode.is_some() {
                        bail!("mode already specified");
                    }
                    mode = Mode::from_str(arg.as_str());
                }
                "--workspace" => {
                    workspace = args.next().map(PathBuf::from);
                }
                "--message" => {
                    message = args.next();
                }
                "--log-dir" => {
                    log_dir = args.next().map(PathBuf::from);
                }
                other => bail!("unknown argument: {other}"),
            }
        }

        let mode = mode.ok_or_else(|| anyhow!("mode is required"))?;
        let message = message.unwrap_or_else(|| format!("integration log {}", mode.as_str()));

        Ok(Self {
            mode,
            workspace,
            message,
            log_dir,
        })
    }

    fn into_invocation(self) -> Result<LogInvocation> {
        match self.mode {
            Mode::Monitor => Ok(LogInvocation::new(LogInvocationKind::Monitor, None)),
            Mode::LocalDev => {
                let workspace = self
                    .workspace
                    .ok_or_else(|| anyhow!("workspace is required for localdev"))?;
                Ok(LogInvocation::new(LogInvocationKind::Run, Some(workspace)))
            }
            Mode::Batch => {
                let workspace = self
                    .workspace
                    .ok_or_else(|| anyhow!("workspace is required for batch"))?;
                Ok(LogInvocation::new(
                    LogInvocationKind::Batch,
                    Some(workspace),
                ))
            }
        }
    }
}

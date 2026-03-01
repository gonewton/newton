use anyhow::{anyhow, bail, Context, Result};
use newton::cli::args::{BatchArgs, MonitorArgs, RunArgs};
use newton::cli::Command;
use newton::logging;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let config = Config::parse()?;
    if let Some(workspace) = &config.workspace {
        env::set_current_dir(workspace)
            .context("failed to set current working directory for helper")?;
    }
    let message = config.message.clone();
    let command = config.into_command()?;

    let _guard =
        logging::init(&command).context("failed to initialize logging from helper binary")?;

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
}

impl Config {
    fn parse() -> Result<Self> {
        let mut args = env::args().skip(1);
        let mut mode = None;
        let mut workspace = None;
        let mut message = None;

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
                other => bail!("unknown argument: {}", other),
            }
        }

        let mode = mode.ok_or_else(|| anyhow!("mode is required"))?;
        let message = message.unwrap_or_else(|| format!("integration log {}", mode.as_str()));

        Ok(Self {
            mode,
            workspace,
            message,
        })
    }

    fn into_command(self) -> Result<Command> {
        match self.mode {
            Mode::Monitor => Ok(Command::Monitor(MonitorArgs {
                http_url: None,
                ws_url: None,
            })),
            Mode::LocalDev => {
                let workspace = self
                    .workspace
                    .ok_or_else(|| anyhow!("workspace is required for localdev"))?;
                Ok(Command::Run(RunArgs {
                    workflow_positional: Some(workspace.join("workflow.yaml")),
                    input_file: None,
                    file: None,
                    workspace: Some(workspace),
                    arg: Vec::new(),
                    set: Vec::new(),
                    trigger_json: None,
                    parallel_limit: None,
                    max_time_seconds: None,
                    verbose: false,
                }))
            }
            Mode::Batch => {
                let workspace = self
                    .workspace
                    .ok_or_else(|| anyhow!("workspace is required for batch"))?;
                Ok(Command::Batch(BatchArgs {
                    project_id: "integration".into(),
                    workspace: Some(workspace),
                    once: true,
                    sleep: 0,
                }))
            }
        }
    }
}

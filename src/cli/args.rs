use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Args, Clone)]
pub struct RunArgs {
    /// Path containing Newton manifests and artifacts
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,

    /// Cap the loop after this many iterations (default: 10)
    #[arg(long, default_value = "10")]
    pub max_iterations: usize,

    /// Abort the loop after this wall-clock budget in seconds (default: 300)
    #[arg(long, default_value = "300")]
    pub max_time: u64,

    /// Replace the default evaluator tool invocation (strict mode)
    #[arg(long, value_name = "CMD", help_heading = "Strict Mode Overrides")]
    pub evaluator_cmd: Option<String>,

    /// Replace the default advisor tool invocation (strict mode)
    #[arg(long, value_name = "CMD", help_heading = "Strict Mode Overrides")]
    pub advisor_cmd: Option<String>,

    /// Replace the default executor tool invocation (strict mode)
    #[arg(long, value_name = "CMD", help_heading = "Strict Mode Overrides")]
    pub executor_cmd: Option<String>,

    /// Custom location for captured evaluator status artifacts
    #[arg(
        long,
        default_value = "artifacts/evaluator_status.md",
        value_name = "FILE",
        help_heading = "Artifact Paths"
    )]
    pub evaluator_status_file: PathBuf,

    /// Custom location for advisor recommendation notes
    #[arg(
        long,
        default_value = "artifacts/advisor_recommendations.md",
        value_name = "FILE",
        help_heading = "Artifact Paths"
    )]
    pub advisor_recommendations_file: PathBuf,

    /// Custom location for executor streaming logs
    #[arg(
        long,
        default_value = "artifacts/executor_log.md",
        value_name = "FILE",
        help_heading = "Artifact Paths"
    )]
    pub executor_log_file: PathBuf,

    /// Default timeout applied to every tool (seconds)
    #[arg(long, default_value = "30", help_heading = "Timeout Overrides")]
    pub tool_timeout_seconds: u64,

    /// Override timeout for evaluator tool only (seconds)
    #[arg(long, value_name = "SECONDS", help_heading = "Timeout Overrides")]
    pub evaluator_timeout: Option<u64>,

    /// Override timeout for advisor tool only (seconds)
    #[arg(long, value_name = "SECONDS", help_heading = "Timeout Overrides")]
    pub advisor_timeout: Option<u64>,

    /// Override timeout for executor tool only (seconds)
    #[arg(long, value_name = "SECONDS", help_heading = "Timeout Overrides")]
    pub executor_timeout: Option<u64>,

    /// Enable verbose output to display tool stdout/stderr
    #[arg(long, help_heading = "Output Options")]
    pub verbose: bool,

    /// Path to custom config file (default: {workspace}/newton.toml)
    #[arg(long, value_name = "FILE", help_heading = "Configuration")]
    pub config: Option<PathBuf>,

    /// Goal description (written to goal file, passed as NEWTON_GOAL_FILE)
    #[arg(long, value_name = "TEXT", help_heading = "Goal Management")]
    pub goal: Option<String>,

    /// Existing goal file to use instead of writing --goal text (passed as NEWTON_GOAL_FILE)
    #[arg(long, value_name = "FILE", help_heading = "Goal Management")]
    pub goal_file: Option<PathBuf>,

    /// Control file path override (default: newton_control.json)
    #[arg(long, value_name = "FILE", help_heading = "Goal Management")]
    pub control_file: Option<PathBuf>,

    /// User feedback passed to tools via NEWTON_USER_FEEDBACK
    #[arg(long, value_name = "TEXT", help_heading = "User Interaction")]
    pub feedback: Option<String>,
}

#[derive(Parser, Clone)]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowCommand,
}

#[derive(Subcommand, Clone)]
pub enum WorkflowCommand {
    #[command(about = "Execute a workflow graph")]
    Run(WorkflowRunArgs),
    #[command(about = "Validate a workflow graph definition")]
    Validate(WorkflowValidateArgs),
    #[command(about = "Render workflow graph as DOT")]
    Dot(WorkflowDotArgs),
}

#[derive(Args, Clone)]
pub struct WorkflowRunArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<KeyValuePair>,

    #[arg(long)]
    pub parallel_limit: Option<usize>,

    #[arg(long)]
    pub max_time_seconds: Option<u64>,
}

#[derive(Args, Clone)]
pub struct WorkflowValidateArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,
}

#[derive(Args, Clone)]
pub struct WorkflowDotArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct KeyValuePair {
    pub key: String,
    pub value: String,
}

impl FromStr for KeyValuePair {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '=');
        let key = parts
            .next()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "missing key".to_string())?;
        let value = parts
            .next()
            .ok_or_else(|| "missing value".to_string())?
            .trim()
            .to_string();
        Ok(KeyValuePair {
            key: key.to_string(),
            value,
        })
    }
}

impl RunArgs {
    /// Produce default arguments used by the batch processor.
    pub fn for_batch(project_root: PathBuf, goal_file: Option<PathBuf>) -> Self {
        RunArgs {
            path: project_root,
            max_iterations: 10,
            max_time: 300,
            evaluator_cmd: None,
            advisor_cmd: None,
            executor_cmd: None,
            evaluator_status_file: PathBuf::from("artifacts/evaluator_status.md"),
            advisor_recommendations_file: PathBuf::from("artifacts/advisor_recommendations.md"),
            executor_log_file: PathBuf::from("artifacts/executor_log.md"),
            tool_timeout_seconds: 30,
            evaluator_timeout: None,
            advisor_timeout: None,
            executor_timeout: None,
            verbose: false,
            config: None,
            goal: None,
            goal_file,
            control_file: None,
            feedback: None,
        }
    }

    /// Produce batch arguments when tooling overrides and limits should come from config.
    #[allow(clippy::too_many_arguments)] // Batch helper mirrors CLI overrides so the argument count reflects user-facing flags.
    pub fn for_batch_with_tools(
        project_root: PathBuf,
        goal_file: Option<PathBuf>,
        evaluator_cmd: Option<String>,
        advisor_cmd: Option<String>,
        executor_cmd: Option<String>,
        max_iterations: Option<usize>,
        max_time: Option<u64>,
        verbose: bool,
        control_file_path: Option<PathBuf>,
    ) -> Self {
        let config = BatchRunConfig {
            project_root,
            goal_file,
            evaluator_cmd,
            advisor_cmd,
            executor_cmd,
            max_iterations,
            max_time,
            verbose,
            control_file_path,
        };
        Self::from_batch_config(config)
    }

    fn from_batch_config(config: BatchRunConfig) -> Self {
        RunArgs {
            path: config.project_root,
            max_iterations: config.max_iterations.unwrap_or(5),
            max_time: config.max_time.unwrap_or(3600),
            evaluator_cmd: config.evaluator_cmd,
            advisor_cmd: config.advisor_cmd,
            executor_cmd: config.executor_cmd,
            evaluator_status_file: PathBuf::from("artifacts/evaluator_status.md"),
            advisor_recommendations_file: PathBuf::from("artifacts/advisor_recommendations.md"),
            executor_log_file: PathBuf::from("artifacts/executor_log.md"),
            tool_timeout_seconds: 30,
            evaluator_timeout: None,
            advisor_timeout: None,
            executor_timeout: None,
            verbose: config.verbose,
            config: None,
            goal: None,
            goal_file: config.goal_file,
            control_file: config.control_file_path,
            feedback: None,
        }
    }
}

/// Configuration for batch run arguments
pub struct BatchRunConfig {
    pub project_root: PathBuf,
    pub goal_file: Option<PathBuf>,
    pub evaluator_cmd: Option<String>,
    pub advisor_cmd: Option<String>,
    pub executor_cmd: Option<String>,
    pub max_iterations: Option<usize>,
    pub max_time: Option<u64>,
    pub verbose: bool,
    pub control_file_path: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct BatchArgs {
    /// Project identifier that maps to .newton/configs/<project_id>.conf
    #[arg(value_name = "PROJECT_ID")]
    pub project_id: String,

    /// Workspace root containing the .newton directory (default: discover from CWD)
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Process a single plan and exit instead of running as a daemon
    #[arg(long)]
    pub once: bool,

    /// Sleep interval in seconds when the queue is empty (default: 60)
    #[arg(long, default_value = "60", value_name = "SECONDS")]
    pub sleep: u64,
}

#[derive(Args)]
pub struct StepArgs {
    /// Path to read/write Newton artifacts from
    #[arg(value_name = "PATH")]
    pub path: PathBuf,

    /// Associate the single step with an execution ID for auditing
    #[arg(long, value_name = "EXECUTION")]
    pub execution_id: Option<String>,

    /// Enable verbose output to display tool stdout/stderr
    #[arg(long, help_heading = "Output Options")]
    pub verbose: bool,
}

#[derive(Args)]
pub struct InitArgs {
    /// Directory where .newton/ will be created (defaults to current directory)
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Template source (GitHub repo, URL, or local path; default: gonewton/newton-templates)
    #[arg(long, value_name = "SOURCE")]
    pub template_source: Option<String>,
}

#[derive(Args)]
pub struct StatusArgs {
    /// Identifier of the execution to inspect
    #[arg(value_name = "EXECUTION")]
    pub execution_id: String,

    /// Path storing the execution ledger
    #[arg(long, default_value = ".", value_name = "PATH")]
    pub path: PathBuf,
}

#[derive(Args)]
pub struct ReportArgs {
    /// Execution whose insights should be summarized
    #[arg(value_name = "EXECUTION")]
    pub execution_id: String,

    /// Path storing source artifacts for the report
    #[arg(long, default_value = ".", value_name = "PATH")]
    pub path: PathBuf,

    /// Emit either terminal-friendly text or machine-readable JSON
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: ReportFormat,
}

#[derive(Clone, clap::ValueEnum, Debug)]
pub enum ReportFormat {
    /// Human-readable, Markdown-friendly summary
    Text,
    /// JSON payload suitable for downstream tooling
    Json,
}

#[derive(Args, Clone, Debug)]
pub struct MonitorArgs {
    /// Override HTTP endpoint for the ailoop server.
    ///
    /// When provided, --ws-url must also be provided (or available in config).
    /// If omitted, HTTP URL is loaded from .newton/configs/monitor.conf or the
    /// first alphabetically-sorted .conf file that defines both endpoints.
    #[arg(long, value_name = "URL")]
    pub http_url: Option<String>,

    /// Override WebSocket endpoint for the ailoop server.
    ///
    /// When provided, --http-url must also be provided (or available in config).
    /// If omitted, WebSocket URL is loaded from .newton/configs/monitor.conf or
    /// the first alphabetically-sorted .conf file that defines both endpoints.
    #[arg(long, value_name = "URL")]
    pub ws_url: Option<String>,
}

#[derive(Args)]
pub struct ErrorArgs {
    /// Execution ID whose failures should be analyzed
    #[arg(value_name = "EXECUTION")]
    pub execution_id: String,

    /// Include stack traces, raw logs, and contextual artifacts
    #[arg(long)]
    pub verbose: bool,
}

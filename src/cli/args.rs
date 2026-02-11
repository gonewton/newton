use clap::Args;
use std::path::PathBuf;

pub const DEFAULT_TEMPLATE_SOURCE: &str = "gonewton/newton-templates";

#[derive(Args)]
pub struct InitArgs {
    /// Path containing the workspace to initialize (default: current directory)
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Template source to install (default: gonewton/newton-templates)
    #[arg(long, value_name = "SOURCE")]
    pub template_source: Option<String>,
}

#[derive(Args)]
pub struct RunArgs {
    /// Path containing Newton manifests and artifacts (defaults to current directory)
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

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

impl RunArgs {
    /// Produce default arguments used by the batch processor.
    pub fn for_batch(project_root: PathBuf, goal_file: Option<PathBuf>) -> Self {
        RunArgs {
            path: Some(project_root),
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
}

#[derive(Args)]
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
    /// Explicit HTTP URL for the ailoop server (default from .newton/configs/)
    #[arg(long, value_name = "URL")]
    pub http_url: Option<String>,

    /// Explicit WebSocket URL for the ailoop server (default from .newton/configs/)
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

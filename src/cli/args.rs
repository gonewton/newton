use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct RunArgs {
    /// Path to workspace directory
    pub path: PathBuf,

    /// Maximum number of optimization iterations
    #[arg(long, default_value = "10")]
    pub max_iterations: usize,

    /// Maximum execution time in seconds
    #[arg(long, default_value = "300")]
    pub max_time: u64,

    /// Command to run for evaluation phase (enables strict mode)
    #[arg(long)]
    pub evaluator_cmd: Option<String>,

    /// Command to run for advice phase (enables strict mode)
    #[arg(long)]
    pub advisor_cmd: Option<String>,

    /// Command to run for execution phase (enables strict mode)
    #[arg(long)]
    pub executor_cmd: Option<String>,

    /// Path to evaluator status output file
    #[arg(long, default_value = "artifacts/evaluator_status.md")]
    pub evaluator_status_file: PathBuf,

    /// Path to advisor recommendations output file
    #[arg(long, default_value = "artifacts/advisor_recommendations.md")]
    pub advisor_recommendations_file: PathBuf,

    /// Path to executor log output file
    #[arg(long, default_value = "artifacts/executor_log.md")]
    pub executor_log_file: PathBuf,

    /// Global timeout for tool execution in seconds
    #[arg(long, default_value = "30")]
    pub tool_timeout_seconds: u64,

    /// Specific timeout for evaluator tool
    #[arg(long)]
    pub evaluator_timeout: Option<u64>,

    /// Specific timeout for advisor tool
    #[arg(long)]
    pub advisor_timeout: Option<u64>,

    /// Specific timeout for executor tool
    #[arg(long)]
    pub executor_timeout: Option<u64>,
}

#[derive(Args)]
pub struct StepArgs {
    /// Path to workspace directory
    pub path: PathBuf,

    /// Optional execution ID for tracking
    #[arg(long)]
    pub execution_id: Option<String>,
}

#[derive(Args)]
pub struct StatusArgs {
    /// ID of the optimization execution
    pub execution_id: String,

    /// Workspace path
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

#[derive(Args)]
pub struct ReportArgs {
    /// ID of the optimization execution
    pub execution_id: String,

    /// Workspace path
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,

    /// Report output format
    #[arg(long, default_value = "text")]
    pub format: ReportFormat,
}

#[derive(Clone, clap::ValueEnum)]
pub enum ReportFormat {
    Text,
    Json,
}

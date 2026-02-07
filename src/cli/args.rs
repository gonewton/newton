use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct RunArgs {
    /// Path containing Newton manifests and artifacts
    #[arg(value_name = "PATH")]
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

    /// Control file path override (default: newton_control.json)
    #[arg(long, value_name = "FILE", help_heading = "Goal Management")]
    pub control_file: Option<PathBuf>,

    /// Branch name to create/checkout
    #[arg(
        long,
        value_name = "NAME",
        help_heading = "Branch Management",
        conflicts_with = "branch_from_goal"
    )]
    pub branch: Option<String>,

    /// Create branch name from goal using branch_namer_cmd
    #[arg(long, help_heading = "Branch Management", conflicts_with = "branch")]
    pub branch_from_goal: bool,

    /// User feedback passed to tools via NEWTON_USER_FEEDBACK
    #[arg(long, value_name = "TEXT", help_heading = "User Interaction")]
    pub feedback: Option<String>,

    /// Restore original branch after completion
    #[arg(long, help_heading = "Git Integration")]
    pub restore_branch: bool,

    /// Create PR on successful completion
    #[arg(long, help_heading = "Git Integration")]
    pub create_pr: bool,
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

#[derive(Args)]
pub struct ErrorArgs {
    /// Execution ID whose failures should be analyzed
    #[arg(value_name = "EXECUTION")]
    pub execution_id: String,

    /// Include stack traces, raw logs, and contextual artifacts
    #[arg(long)]
    pub verbose: bool,
}

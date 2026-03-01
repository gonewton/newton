use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Args, Clone)]
pub struct RunArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow: PathBuf,

    /// Optional path written into triggers.payload.input_file
    #[arg(value_name = "INPUT_FILE")]
    pub input_file: Option<PathBuf>,

    /// Workspace root directory (default: current directory)
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Merge KEY into triggers.payload; VALUE may be @path to read from file, @@ for literal @
    #[arg(long, value_name = "KEY=VALUE")]
    pub arg: Vec<KeyValuePair>,

    /// Merge KEY into workflow.context at runtime
    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<KeyValuePair>,

    /// Load JSON object as base trigger payload before --arg overrides
    #[arg(long, value_name = "PATH")]
    pub trigger_json: Option<PathBuf>,

    /// Runtime override for bounded task concurrency
    #[arg(long, value_name = "N")]
    pub parallel_limit: Option<usize>,

    /// Runtime wall-clock limit override (seconds)
    #[arg(long, value_name = "N")]
    pub max_time_seconds: Option<u64>,

    /// Print task stdout/stderr to terminal after each task completes
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Args, Clone)]
pub struct WebhookArgs {
    #[command(subcommand)]
    pub command: WebhookCommand,
}

#[derive(Subcommand, Clone)]
pub enum WebhookCommand {
    #[command(about = "Serve a webhook listener")]
    Serve(WebhookServeArgs),
    #[command(about = "Show webhook configuration status")]
    Status(WebhookStatusArgs),
}

#[derive(Args, Clone)]
pub struct WebhookServeArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,
}

#[derive(Args, Clone)]
pub struct WebhookStatusArgs {
    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub workflow: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Args, Clone)]
pub struct LintArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Args, Clone)]
pub struct ExplainArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<KeyValuePair>,

    /// Trigger payload override in KEY=VALUE form (supports VALUE=@path)
    #[arg(long, value_name = "KEY=VALUE")]
    pub arg: Vec<KeyValuePair>,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Path to JSON file containing manual trigger payload
    #[arg(long, value_name = "PATH")]
    pub trigger_json: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ValidateArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,
}

#[derive(Args, Clone)]
pub struct DotArgs {
    #[arg(long, value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ResumeArgs {
    #[arg(long, value_name = "UUID")]
    pub execution_id: Uuid,

    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    #[arg(long)]
    pub allow_workflow_change: bool,
}

#[derive(Args, Clone)]
pub struct CheckpointsArgs {
    #[command(subcommand)]
    pub command: CheckpointCommand,
}

#[derive(Subcommand, Clone)]
pub enum CheckpointCommand {
    #[command(about = "List workflow checkpoints")]
    List {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long)]
        format_json: bool,
    },
    #[command(about = "Clean historical checkpoints")]
    Clean {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long, value_name = "DURATION")]
        older_than: String,
    },
}

#[derive(Args, Clone)]
pub struct ArtifactsArgs {
    #[command(subcommand)]
    pub command: ArtifactCommand,
}

#[derive(Subcommand, Clone)]
pub enum ArtifactCommand {
    #[command(about = "Clean artifact store files")]
    Clean {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long, value_name = "DURATION")]
        older_than: String,
    },
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

// StepArgs removed - command retired

#[derive(Args)]
pub struct InitArgs {
    /// Directory where .newton/ will be created (defaults to current directory)
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Template source (GitHub repo, URL, or local path; default: gonewton/newton-templates)
    #[arg(long, value_name = "SOURCE")]
    pub template_source: Option<String>,
}

// StatusArgs removed - command retired

// ReportArgs and ReportFormat removed - commands retired

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

// ErrorArgs removed - command retired

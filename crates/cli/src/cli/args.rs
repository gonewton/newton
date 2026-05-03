use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "LOG-003: --last must be a positive integer".to_string())?;
    if parsed == 0 {
        Err("LOG-003: --last must be a positive integer".to_string())
    } else {
        Ok(parsed)
    }
}

#[derive(Args, Clone)]
pub struct LogArgs {
    #[command(subcommand)]
    pub command: LogCommand,
}

#[derive(Subcommand, Clone)]
pub enum LogCommand {
    #[command(
        about = "List workflow execution history for a workspace",
        after_help = "EXAMPLES:\n  List recent runs for a workspace:\n    newton log list --workspace ./workspace\n\n  Last 10 runs as JSON:\n    newton log list --last 10 --json"
    )]
    List {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,
        /// Only list the N most recent executions (after sort by started_at desc)
        #[arg(long, value_name = "N", value_parser = parse_positive_usize)]
        last: Option<usize>,
        /// Emit machine-readable JSON (stable keys per spec §4.2)
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Replay task-by-task execution detail for a specific run",
        after_help = "EXAMPLES:\n  Show full execution detail:\n    newton log show <execution-id> --workspace ./workspace\n\n  Filter to a single task with verbose output:\n    newton log show <execution-id> --task my-task --verbose"
    )]
    Show {
        #[arg(value_name = "EXECUTION_ID")]
        execution_id: Uuid,
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,
        /// Filter output to a single task ID
        #[arg(long, value_name = "TASK_ID")]
        task: Option<String>,
        /// Expand single-task output for debugging (only effective with --task)
        #[arg(short, long)]
        verbose: bool,
        /// Emit machine-readable JSON (stable keys per spec §4.2)
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args, Clone)]
pub struct RunArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW", index = 1)]
    pub workflow_positional: Option<PathBuf>,

    /// Optional path written into triggers.payload.input_file
    #[arg(value_name = "INPUT_FILE", index = 2)]
    pub input_file: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

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

    /// Newton server URL to register this run (optional)
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,
}

impl RunArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Args, Clone)]
pub struct WebhookArgs {
    #[command(subcommand)]
    pub command: WebhookCommand,
}

#[derive(Subcommand, Clone)]
pub enum WebhookCommand {
    #[command(
        about = "Start an HTTP server to receive webhook events and trigger workflows",
        after_help = "EXAMPLES:\n  Serve a workflow with positional argument:\n    newton webhook serve workflow.yaml --workspace ./workspace\n\n  Serve a workflow specified via --file:\n    newton webhook serve --file ./workflows/deploy.yaml --workspace ./project"
    )]
    Serve(WebhookServeArgs),
    #[command(
        about = "Display webhook endpoint configuration and server status",
        after_help = "EXAMPLES:\n  Show webhook status for a workflow:\n    newton webhook status workflow.yaml --workspace ./workspace"
    )]
    Status(WebhookStatusArgs),
}

#[derive(Args, Clone)]
pub struct WebhookServeArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,
}

impl WebhookServeArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Args, Clone)]
pub struct WebhookStatusArgs {
    /// Path to the workflow YAML file (optional)
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,
}

impl WebhookStatusArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Prose,
}

#[derive(Args, Clone)]
pub struct LintArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

impl LintArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Args, Clone)]
pub struct ExplainArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

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

impl ExplainArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Args, Clone)]
pub struct ValidateArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,
}

impl ValidateArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
}

#[derive(Args, Clone)]
pub struct DotArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow_positional: Option<PathBuf>,

    /// Path to the workflow YAML file (alternative to positional)
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

impl DotArgs {
    /// Resolve workflow path with precedence: --file over positional
    pub fn resolved_workflow_path(&self) -> Option<PathBuf> {
        self.file
            .clone()
            .or_else(|| self.workflow_positional.clone())
    }
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
    #[command(
        about = "Display available workflow executions and their checkpoint details",
        after_help = "EXAMPLES:\n  List checkpoints in a workspace:\n    newton checkpoints list --workspace ./workspace\n\n  List checkpoints as JSON:\n    newton checkpoints list --workspace ./workspace --format-json"
    )]
    List {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long)]
        format_json: bool,
    },
    #[command(
        about = "Remove old checkpoint files to free up disk space",
        after_help = "EXAMPLES:\n  Remove checkpoints older than 7 days:\n    newton checkpoints clean --workspace ./workspace --older-than 7d"
    )]
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
    #[command(
        about = "Remove old workflow output files and execution artifacts",
        after_help = "EXAMPLES:\n  Remove artifacts older than 30 days:\n    newton artifacts clean --workspace ./workspace --older-than 30d"
    )]
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

    /// Also start the HTTP API backend server
    #[arg(long)]
    pub backend: bool,
}

#[derive(Args, Clone, Debug)]
pub struct ServeArgs {
    /// Host address to bind the server to (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on (default: 8080)
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Path to the built Newton UI dist directory (optional)
    #[arg(long, value_name = "PATH")]
    pub ui_dir: Option<PathBuf>,
}

// ErrorArgs removed - command retired
// ErrorArgs removed - command retired

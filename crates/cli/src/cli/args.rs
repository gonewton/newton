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

// ── Runs (was Log) ────────────────────────────────────────────────────────────

#[derive(Args, Clone)]
pub struct RunsArgs {
    #[command(subcommand)]
    pub command: RunsCommand,
}

#[derive(Subcommand, Clone)]
pub enum RunsCommand {
    #[command(
        about = "List workflow execution history for a workspace",
        after_help = "EXAMPLES:\n  newton runs list --workspace ./workspace\n  newton runs list --last 10 --json"
    )]
    List {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,
        /// Only list the N most recent executions (after sort by started_at desc)
        #[arg(long, value_name = "N", value_parser = parse_positive_usize)]
        last: Option<usize>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Replay task-by-task execution detail for a specific run",
        after_help = "EXAMPLES:\n  newton runs show <run-id>\n  newton runs show <run-id> --task my-task --verbose"
    )]
    Show {
        /// Run identifier (UUID)
        #[arg(value_name = "RUN_ID")]
        run_id: Uuid,
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,
        /// Filter output to a single task ID
        #[arg(long, value_name = "TASK_ID")]
        task: Option<String>,
        /// Expand single-task output for debugging
        #[arg(short, long)]
        verbose: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

// ── Run ───────────────────────────────────────────────────────────────────────

#[derive(Args, Clone)]
pub struct RunArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW", index = 1)]
    pub workflow: PathBuf,

    /// Optional path written into triggers.payload.input_file
    #[arg(value_name = "INPUT_FILE", index = 2)]
    pub input_file: Option<PathBuf>,

    /// Workspace root directory (default: current directory)
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Merge KEY into trigger payload; VALUE may be @path to read from file, @@ for literal @
    #[arg(long = "trigger", value_name = "KEY=VALUE")]
    pub trigger: Vec<KeyValuePair>,

    /// Merge KEY into workflow.context at runtime
    #[arg(long = "context", value_name = "KEY=VALUE")]
    pub context: Vec<KeyValuePair>,

    /// Load JSON object as base parameters before --trigger overrides.
    /// Accepts a bare path or @path syntax.
    #[arg(long = "parameters-json", value_name = "PATH")]
    pub parameters_json: Option<PathBuf>,

    /// Write structured completion envelope to stdout as JSON.
    #[arg(long = "emit-completion-json")]
    pub emit_completion_json: bool,

    /// Runtime override for bounded task concurrency
    #[arg(long, value_name = "N")]
    pub parallel_limit: Option<usize>,

    /// Runtime wall-clock limit override (seconds)
    #[arg(long = "timeout", value_name = "SECONDS")]
    pub timeout_seconds: Option<u64>,

    /// Print task stdout/stderr to terminal after each task completes
    #[arg(short, long)]
    pub verbose: bool,

    /// Newton server URL to register this run (optional)
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,
}

// ── Webhook ───────────────────────────────────────────────────────────────────

#[derive(Args, Clone)]
pub struct WebhookArgs {
    #[command(subcommand)]
    pub command: WebhookCommand,
}

#[derive(Subcommand, Clone)]
pub enum WebhookCommand {
    #[command(
        about = "Start an HTTP server to receive webhook events and trigger workflows",
        after_help = "EXAMPLES:\n  newton webhook serve --workflow ./workflows/deploy.yaml --workspace ./project"
    )]
    Serve(WebhookServeArgs),
    #[command(
        about = "Display webhook endpoint configuration and server status",
        after_help = "EXAMPLES:\n  newton webhook status --workflow workflow.yaml --workspace ./workspace"
    )]
    Status(WebhookStatusArgs),
}

#[derive(Args, Clone)]
pub struct WebhookServeArgs {
    /// Path to the workflow YAML file
    #[arg(long = "workflow", value_name = "PATH")]
    pub workflow: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,
}

#[derive(Args, Clone)]
pub struct WebhookStatusArgs {
    /// Path to the workflow YAML file (optional)
    #[arg(long = "workflow", value_name = "PATH")]
    pub workflow: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub workspace: PathBuf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Prose,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "lowercase")]
pub enum GraphFormat {
    #[default]
    Dot,
}

// ── Workflow group ────────────────────────────────────────────────────────────

#[derive(Args, Clone)]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowCommand,
}

#[derive(Subcommand, Clone)]
pub enum WorkflowCommand {
    #[command(about = "Validate a workflow graph definition")]
    Validate(ValidateArgs),
    #[command(about = "Check workflow for best practices and potential issues")]
    Lint(LintArgs),
    #[command(about = "Preview what the workflow will do at runtime")]
    Preview(ExplainArgs),
    #[command(about = "Render the workflow graph (dot output)")]
    Graph(DotArgs),
    #[command(
        about = "Execute a workflow graph",
        long_about = crate::cli::framework_setup::WORKFLOW_RUN_LONG_ABOUT
    )]
    Run(RunArgs),
}

#[derive(Args, Clone)]
pub struct LintArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow: PathBuf,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Args, Clone)]
pub struct ExplainArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow: PathBuf,

    /// Workspace root directory (default: current directory)
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Merge KEY into workflow.context at runtime
    #[arg(long = "context", value_name = "KEY=VALUE")]
    pub context: Vec<KeyValuePair>,

    /// Trigger payload override in KEY=VALUE form (supports VALUE=@path)
    #[arg(long = "trigger", value_name = "KEY=VALUE")]
    pub trigger: Vec<KeyValuePair>,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Path to JSON file containing manual trigger payload (base).
    /// Accepts a bare path or @path syntax.
    #[arg(long = "parameters-json", value_name = "PATH")]
    pub parameters_json: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ValidateArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow: PathBuf,
}

#[derive(Args, Clone)]
pub struct DotArgs {
    /// Path to the workflow YAML file
    #[arg(value_name = "WORKFLOW")]
    pub workflow: PathBuf,

    /// Output graph format (currently only `dot` is supported)
    #[arg(long, value_enum, default_value = "dot")]
    pub format: GraphFormat,

    /// Output destination file (defaults to stdout)
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    pub output: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ResumeArgs {
    /// Run identifier (UUID) of the workflow execution to resume
    #[arg(long = "run-id", value_name = "UUID")]
    pub run_id: Uuid,

    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    #[arg(long)]
    pub allow_workflow_change: bool,
}

#[derive(Args, Clone)]
pub struct CheckpointArgs {
    #[command(subcommand)]
    pub command: CheckpointCommand,
}

#[derive(Subcommand, Clone)]
pub enum CheckpointCommand {
    #[command(
        about = "Display available workflow executions and their checkpoint details",
        after_help = "EXAMPLES:\n  newton checkpoint list --workspace ./workspace\n  newton checkpoint list --workspace ./workspace --json"
    )]
    List {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long = "json")]
        json: bool,
    },
    #[command(
        about = "Remove old checkpoint files to free up disk space",
        after_help = "EXAMPLES:\n  newton checkpoint clean --workspace ./workspace --older-than 7d"
    )]
    Clean {
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        #[arg(long, value_name = "DURATION")]
        older_than: String,
    },
}

#[derive(Args, Clone)]
pub struct ArtifactArgs {
    #[command(subcommand)]
    pub command: ArtifactCommand,
}

#[derive(Subcommand, Clone)]
pub enum ArtifactCommand {
    #[command(
        about = "Remove old workflow output files and execution artifacts",
        after_help = "EXAMPLES:\n  newton artifact clean --workspace ./workspace --older-than 30d"
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

    /// Seconds to wait when the queue is empty (default: 60)
    #[arg(long = "poll-interval", default_value = "60", value_name = "SECONDS")]
    pub poll_interval_seconds: u64,
}

#[derive(Args)]
pub struct InitArgs {
    /// Directory where .newton/ will be created (defaults to current directory)
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Template source (GitHub repo, URL, or local path; default: gonewton/newton-templates)
    #[arg(long = "template", value_name = "SOURCE")]
    pub template: Option<String>,
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
    #[arg(long = "static-ui", value_name = "PATH")]
    pub static_ui: Option<PathBuf>,

    /// Mount the MCP HTTP router on the same listener as the Newton API.
    #[arg(long = "with-mcp", default_value_t = false)]
    pub with_mcp: bool,

    /// Path prefix at which the MCP HTTP router is mounted (used only with --with-mcp).
    #[arg(long = "mcp-path", default_value = "/mcp")]
    pub mcp_path: String,

    /// Embed the ailoop HTTP/WebSocket server on the same listener as the Newton API.
    #[arg(long = "with-embedded-ailoop", default_value_t = false)]
    pub with_embedded_ailoop: bool,

    /// Path prefix at which the embedded ailoop router is mounted
    /// (used only with --with-embedded-ailoop). Must not be `/api`.
    #[arg(long = "ailoop-base-path", default_value = "/ailoop")]
    pub ailoop_base_path: String,
}

// ── Data ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataVerb {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl DataVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            DataVerb::Get => "get",
            DataVerb::Post => "post",
            DataVerb::Put => "put",
            DataVerb::Patch => "patch",
            DataVerb::Delete => "delete",
        }
    }
}

impl std::fmt::Display for DataVerb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct DataArgs {
    pub verb: DataVerb,
    pub resource: String,
    pub id: Option<String>,
    pub file: Option<PathBuf>,
    pub body: Option<String>,
    pub json: bool,
    pub dry_run: bool,
    pub workspace: Option<PathBuf>,
}

use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

// ── Runs (was Log) ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RunsArgs {
    pub command: RunsCommand,
}

#[derive(Clone)]
pub enum RunsCommand {
    List {
        workspace: Option<PathBuf>,
        /// Only list the N most recent executions (after sort by started_at desc)
        last: Option<usize>,
        /// Emit machine-readable JSON
        json: bool,
        /// Override the state root directory where checkpoints/executions are
        /// stored. Defaults to auto-resolved from workspace root.
        state_dir: Option<PathBuf>,
    },
    Show {
        /// Run identifier (UUID)
        run_id: Uuid,
        workspace: Option<PathBuf>,
        /// Filter output to a single task ID
        task: Option<String>,
        /// Expand single-task output for debugging
        verbose: bool,
        /// Emit machine-readable JSON
        json: bool,
        /// Override the state root directory where checkpoints/executions are
        /// stored. Defaults to auto-resolved from workspace root.
        state_dir: Option<PathBuf>,
    },
}

// ── Run ───────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RunArgs {
    /// Path to the workflow YAML file
    pub workflow: PathBuf,

    /// Optional path written into triggers.payload.input_file
    pub input_file: Option<PathBuf>,

    /// Workspace root directory (default: current directory)
    pub workspace: Option<PathBuf>,

    /// Merge KEY into trigger payload; VALUE may be @path to read from file, @@ for literal @
    pub trigger: Vec<KeyValuePair>,

    /// Merge KEY into workflow.context at runtime
    pub context: Vec<KeyValuePair>,

    /// Load JSON object as base parameters before --trigger overrides.
    /// Accepts a bare path or @path syntax.
    pub parameters_json: Option<PathBuf>,

    /// Write structured completion envelope to stdout as JSON.
    pub emit_completion_json: bool,

    /// Runtime override for bounded task concurrency
    pub parallel_limit: Option<usize>,

    /// Runtime wall-clock limit override (seconds)
    pub timeout_seconds: Option<u64>,

    /// Print task stdout/stderr to terminal after each task completes
    pub verbose: bool,

    /// Newton server URL to register this run (optional)
    pub server: Option<String>,

    /// Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.
    pub state_dir: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Prose,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum GraphFormat {
    #[default]
    Dot,
}

// ── Workflow group ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct WorkflowArgs {
    pub command: WorkflowCommand,
}

#[derive(Clone)]
pub enum WorkflowCommand {
    Validate(ValidateArgs),
    Lint(LintArgs),
    Preview(ExplainArgs),
    Graph(DotArgs),
    Run(RunArgs),
    Import(ImportArgs),
}

#[derive(Clone)]
pub struct ImportArgs {
    /// Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored.
    pub state_dir: Option<PathBuf>,

    /// Workspace root to scan for existing runs (default: CWD)
    pub workspace: Option<PathBuf>,

    /// Recursively walk workspace for all .newton/state/workflows directories
    pub recursive: bool,
}

#[derive(Clone)]
pub struct LintArgs {
    /// Path to the workflow YAML file
    pub workflow: PathBuf,

    pub format: OutputFormat,
}

#[derive(Clone)]
pub struct ExplainArgs {
    /// Path to the workflow YAML file
    pub workflow: PathBuf,

    /// Workspace root directory (default: current directory)
    pub workspace: Option<PathBuf>,

    /// Merge KEY into workflow.context at runtime
    pub context: Vec<KeyValuePair>,

    /// Trigger payload override in KEY=VALUE form (supports VALUE=@path)
    pub trigger: Vec<KeyValuePair>,

    pub format: OutputFormat,

    /// Path to JSON file containing manual trigger payload (base).
    /// Accepts a bare path or @path syntax.
    pub parameters_json: Option<PathBuf>,
}

#[derive(Clone)]
pub struct ValidateArgs {
    /// Path to the workflow YAML file
    pub workflow: PathBuf,
}

#[derive(Clone)]
pub struct DotArgs {
    /// Path to the workflow YAML file
    pub workflow: PathBuf,

    /// Output graph format (currently only `dot` is supported)
    pub format: GraphFormat,

    /// Output destination file (defaults to stdout)
    pub output: Option<PathBuf>,
}

#[derive(Clone)]
pub struct ResumeArgs {
    /// Run identifier (UUID) of the workflow execution to resume
    pub run_id: Uuid,

    pub workspace: Option<PathBuf>,

    pub allow_workflow_change: bool,

    /// Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.
    pub state_dir: Option<PathBuf>,

    /// Write structured completion envelope to stdout as JSON (parity with `run`)
    pub emit_completion_json: bool,

    /// Print task stdout/stderr to terminal after each task completes (parity with `run`)
    pub verbose: bool,
}

#[derive(Clone)]
pub struct CheckpointArgs {
    pub command: CheckpointCommand,
}

#[derive(Clone)]
pub enum CheckpointCommand {
    List {
        workspace: Option<PathBuf>,

        state_dir: Option<PathBuf>,

        json: bool,
    },
    Clean {
        workspace: Option<PathBuf>,

        state_dir: Option<PathBuf>,

        older_than: String,
    },
}

#[derive(Clone)]
pub struct ArtifactArgs {
    pub command: ArtifactCommand,
}

#[derive(Clone)]
pub enum ArtifactCommand {
    Clean {
        workspace: Option<PathBuf>,

        state_dir: Option<PathBuf>,

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

#[derive(Clone)]
pub struct OptimizeArgs {
    /// Project identifier that maps to .newton/configs/<project_id>.conf
    pub project_id: String,

    /// Workspace root containing the .newton directory (default: discover from CWD)
    pub workspace: Option<PathBuf>,

    /// Process a single Plan and exit instead of running as a daemon
    pub once: bool,

    /// Seconds to wait when the Plan queue is empty (default: 60)
    pub poll_interval_seconds: u64,
}

pub struct InitArgs {
    /// Directory where .newton/ will be created (defaults to current directory)
    pub path: Option<PathBuf>,

    /// Template source (GitHub repo, URL, or local path; default: gonewton/newton-templates)
    pub template: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ServeArgs {
    /// Host address to bind the server to (default: 127.0.0.1)
    pub host: String,

    /// Port to listen on (default: 8080)
    pub port: u16,

    /// Disable serving the embedded web UI (API-only). By default `newton serve`
    /// serves the UI compiled into the binary at all non-API paths.
    pub no_web: bool,

    /// Mount the MCP HTTP router on the same listener as the Newton API.
    pub with_mcp: bool,

    /// Embed the ailoop HTTP/WebSocket server on the same listener as the Newton API.
    pub with_embedded_ailoop: bool,

    /// Path prefix at which the embedded ailoop router is mounted
    /// (used only with --with-embedded-ailoop). Must not be `/api`.
    pub ailoop_base_path: String,

    /// Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.
    pub state_dir: Option<PathBuf>,

    /// Run import scan of existing file-based runs before the HTTP listener binds.
    pub import_existing: bool,

    /// Mount the magic-tool router (`/aitools/...`). Off by default: today it
    /// registers only a `newton/ping` smoke-test tool, with real tool
    /// definitions landing in a future release (spec 074 P9). Not reflected
    /// in the OpenAPI doc until then.
    pub with_magic_tools: bool,
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
    /// Override the state root directory where backend.sqlite is stored.
    /// Defaults to auto-resolved from workspace root (flag > NEWTON_STATE_DIR
    /// > newton.toml [workflow].state_dir > workspace default).
    pub state_dir: Option<PathBuf>,
    /// Optional filter: restrict grade listings to a single evaluation run,
    /// or restrict an `optimize-cycle` GET to the cycle's owning run.
    pub run_id: Option<String>,
    /// Optional filter: restrict grade listings to a single KPI.
    pub kpi_id: Option<String>,
    /// Optional filter: restrict eval-run/finding/plan listings by scope.
    pub scope: Option<String>,
    /// Optional filter: restrict eval-run/finding/plan listings by scope id.
    pub scope_id: Option<String>,
    /// Optional filter: restrict eval run listings by source.
    pub source: Option<String>,
    /// Optional filter: restrict eval run listings to N results.
    pub limit: Option<u32>,
    /// Optional filter: restrict finding/change-request/plan listings by
    /// status (spec 074 P12 — these `Option`s already existed in the store
    /// API; this is the CLI flag that plumbs them through).
    pub status: Option<String>,
}

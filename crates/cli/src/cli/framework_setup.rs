//! cli-framework registration for Newton CLI (Issue #228 Stage 2).
//!
//! All Command / CommandSpec / ArgSpec declarations and the `build_app()`
//! entry point used by `crates/cli/src/main.rs` live here.
//!
//! ## Nested-command note
//! The framework routes via its root-level `commands` map; nested paths in
//! `tree_commands` are not yet dispatched by the clap adapter.  Group
//! commands (checkpoints, artifacts, webhook, log) are therefore registered
//! at root level and dispatch internally via their first positional arg.
//! Proper hierarchical routing is deferred to Stage 3/4 once the framework
//! adds nested-subcommand clap support.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::app::{App, AppBuilder};
use cli_framework::command::{Command, CommandArgs};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use uuid::Uuid;

use crate::cli::args::{
    ArtifactCommand, ArtifactsArgs, BatchArgs, CheckpointCommand, CheckpointsArgs, DotArgs,
    ExplainArgs, InitArgs, LintArgs, LogArgs, LogCommand, MonitorArgs, OutputFormat, ResumeArgs,
    RunArgs, ServeArgs, ValidateArgs, WebhookArgs, WebhookCommand, WebhookServeArgs,
    WebhookStatusArgs,
};
use crate::cli::categories;
use crate::cli::context::NewtonContext;
use crate::cli::ops;
use crate::cli::{commands, init};

#[cfg(feature = "ask")]
use crate::cli::ask;

/// Stable error codes for the migration adapter layer (spec §6).
pub mod error_codes {
    pub const CLI_MIG_001: &str = "CLI-MIG-001";
    pub const CLI_MIG_002: &str = "CLI-MIG-002";
    pub const CLI_MIG_003: &str = "CLI-MIG-003";
    pub const CLI_MIG_004: &str = "CLI-MIG-004";
    pub const CLI_MIG_005: &str = "CLI-MIG-005";
    /// MCP-mode bind failure (issue #237).
    pub const NEWTON_MCP_001: &str = "NEWTON-MCP-001";
    /// MCP-mode upstream runtime error after successful bind (issue #237).
    pub const NEWTON_MCP_002: &str = "NEWTON-MCP-002";
}

// ── help-text constants ───────────────────────────────────────────────────────

const RUN_LONG_ABOUT: &str = "\
Run executes a workflow graph defined in YAML, with optional trigger payload \
from input file or arguments.

Accepted forms:
  newton run [WORKFLOW] [INPUT_FILE] [OPTIONS]
  newton run --file <PATH> [OPTIONS]

EXAMPLES:
  Basic workflow execution:
    newton run workflow.yaml

  With workspace and trigger data:
    newton run workflow.yaml --workspace ./output --arg key=value

  Multiple arguments:
    newton run workflow.yaml --arg env=prod --arg version=1.2.3

  With input file and verbose output:
    newton run workflow.yaml input.txt --workspace ./workspace --verbose";

const INIT_LONG_ABOUT: &str = "\
Init creates the .newton workspace layout, installs the Newton template with \
aikit-sdk, and writes default configs so you can run immediately.

EXAMPLES:
  Initialize current directory:
    newton init .

  Initialize a specific directory:
    newton init ./workspace

  Initialize with custom template source:
    newton init . --template-source gonewton/newton-templates";

const BATCH_LONG_ABOUT: &str = "\
Batch reads plan files from .newton/plan/<project_id> and drives headless \
workflow orchestration.

EXAMPLES:
  Process queued plans for a project:
    newton batch project-alpha

  With workspace override:
    newton batch project-alpha --workspace ./workspace

  Process one plan and exit:
    newton batch project-alpha --once

  Custom poll interval:
    newton batch project-alpha --sleep 30";

// Combined serve long_about: route-group description + examples block.
// The framework renders only long_about in --help, so both must be here.
const SERVE_LONG_ABOUT: &str = "\
Serve runs the Newton HTTP/WebSocket API for UIs, agents, and integrations.
Full REST contract: openapi/newton-backend-parity.yaml.
Schemas and query parameters: skill/newton/references/serve-api.md.

Mounted route groups (see OpenAPI for exact methods, params, and bodies):
  \u{2022} Workflows       /api/workflows, /api/workflows/{id}, /api/workflows/{id}/nodes/{node_id}
  \u{2022} HIL             /api/hil/instances, /api/hil/workflows/{id}
  \u{2022} Streaming       /api/stream/workflow/{id}/{ws,sse}, /api/stream/logs/{id}/{node_id}/ws
  \u{2022} Operators       /api/operators
  \u{2022} Dashboard       /api/products, /api/components, /api/pending-approvals,
                    /api/regressions, /api/indicators, /api/recent-actions
  \u{2022} Portfolio       /api/repos, /api/repo-dependencies, /api/module-dependencies,
                    /api/saved-views
  \u{2022} Opportunities   /api/opportunities, /api/opportunities/{id}
  \u{2022} Requests        /api/requests
  \u{2022} Plans           /api/plans, /api/plans/{id}/{approve,reject}, /api/executions
  \u{2022} Persistence     /api/persistence/{key}
  \u{2022} Testing reset   /api/testing/reset
Always available: GET /health.

CORS is enabled for local development by default.

EXAMPLES:
  Start API server on default port:
    newton serve

  Start on custom host and port:
    newton serve --host 0.0.0.0 --port 9000

  Run in background:
    newton serve --host 0.0.0.0 --port 8080 &

See openapi/newton-backend-parity.yaml for the full HTTP/WebSocket/SSE contract.";

const MONITOR_LONG_ABOUT: &str = "\
Monitor listens to every project/branch channel from the workspace using a \
WebSocket/HTTP mix and lets you answer questions or approve authorizations in a queue.

CONFIGURATION:
  Monitor requires both HTTP and WebSocket endpoints to connect to the ailoop server.
  Endpoints can come from CLI overrides (--http-url, --ws-url) or workspace config files.
  Partial overrides are supported: one flag can be set while the other comes from config.

Endpoint discovery order:
    1. CLI overrides: --http-url and --ws-url (merged with config if partial)
    2. .newton/configs/monitor.conf (if present)
    3. First alphabetical .conf file in .newton/configs/ containing both keys

Config files use simple key=value format:
  ailoop_server_http_url = http://127.0.0.1:8081
  ailoop_server_ws_url = ws://127.0.0.1:8080

EXAMPLES:
  Using both CLI overrides:
    newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080

  Using .newton/configs/monitor.conf:
    newton monitor

  Partial override (HTTP from CLI, WS from config):
    newton monitor --http-url http://192.168.1.10:8081

TROUBLESHOOTING:
  Missing URL configuration:
    If both endpoints are not found, ensure .newton/configs/monitor.conf exists
    or provide both --http-url and --ws-url on the command line.";

const VALIDATE_LONG_ABOUT: &str = "\
Validate checks your workflow YAML file for syntax errors, schema compliance, \
and logical issues before execution.

Accepted forms:
  newton validate [WORKFLOW]
  newton validate --file <PATH>

EXAMPLES:
  Validate a workflow file:
    newton validate workflow.yaml

  Validate with alternative syntax:
    newton validate --file ./workflows/my-workflow.yaml

RETURN CODES:
  0: Workflow is valid and ready to run
  1: Validation errors found (details printed to stderr)";

const DOT_LONG_ABOUT: &str = "\
Dot creates a Graphviz DOT file from your workflow definition that can be \
rendered into visual diagrams.

This command analyzes your workflow's task dependencies and generates a directed \
graph showing task execution flow.

Accepted forms:
  newton dot [WORKFLOW]
  newton dot --file <PATH>

EXAMPLES:
  Generate DOT file to stdout:
    newton dot workflow.yaml

  Save DOT file for rendering:
    newton dot workflow.yaml --out graph.dot

  Create PNG diagram (requires Graphviz):
    newton dot workflow.yaml --out graph.dot && dot -Tpng graph.dot -o workflow.png

VISUALIZATION:
  Use online Graphviz viewers or install Graphviz locally.";

const LINT_LONG_ABOUT: &str = "\
Lint analyzes your workflow definition against Newton's best practices and \
coding standards to identify potential issues.

Unlike validate (which checks syntax), lint focuses on quality and best practices. \
All lint warnings are advisory and won't prevent workflow execution.

Accepted forms:
  newton lint [WORKFLOW]
  newton lint --file <PATH>

EXAMPLES:
  Check workflow with human-readable output:
    newton lint workflow.yaml

  Generate JSON report for CI/CD integration:
    newton lint workflow.yaml --format json

OUTPUT FORMATS:
  text: Human-readable summary (default)
  json: Machine-readable structured data for tooling integration";

const EXPLAIN_LONG_ABOUT: &str = "\
Explain creates detailed documentation about what your workflow does and how \
it will execute.

This command analyzes your workflow definition and produces explanations covering:
  \u{2022} Step-by-step execution flow
  \u{2022} Task dependencies and timing
  \u{2022} Configuration settings and their effects
  \u{2022} Expected inputs and outputs

Accepted forms:
  newton explain [WORKFLOW]
  newton explain --file <PATH>

EXAMPLES:
  Generate structured explanation:
    newton explain workflow.yaml --format text

  Create natural language description:
    newton explain workflow.yaml --format prose

  Explain with custom trigger data:
    newton explain workflow.yaml --arg env=production --format prose

OUTPUT FORMATS:
  text: Structured technical breakdown
  prose: Natural language description
  json: Machine-readable analysis for documentation generation";

const RESUME_LONG_ABOUT: &str = "\
Resume restarts a workflow execution from its last saved checkpoint, allowing \
you to continue from where it left off after an interrupted execution.

EXAMPLES:
  Resume a specific workflow execution:
    newton resume --execution-id 12345678-1234-1234-1234-123456789abc

  Resume with custom workspace:
    newton resume --execution-id abcdef01-2345-6789-abcd-ef0123456789 --workspace ./project

  Resume and allow workflow definition changes:
    newton resume --execution-id 12345678-1234-1234-1234-123456789abc --allow-workflow-change

FINDING EXECUTION IDs:
  List available executions to resume:
    newton checkpoints list --workspace ./workspace

SAFETY:
  By default, resume requires the workflow definition to be unchanged since the checkpoint.
  Use --allow-workflow-change to override this safety check.";

const CHECKPOINTS_LONG_ABOUT: &str = "\
Checkpoints provides tools to manage the saved states that allow workflow \
resumption after interruption.

Newton automatically creates checkpoints during workflow execution to preserve \
progress and enable recovery.

Subcommands:
  list   Display available workflow executions and their checkpoint details
  clean  Remove old checkpoint files to free up disk space

EXAMPLES:
  List all available checkpoints:
    newton checkpoints list --workspace ./workspace

  Get checkpoint details in JSON format:
    newton checkpoints list --workspace ./workspace --format-json

  Clean old checkpoints (older than 7 days):
    newton checkpoints clean --workspace ./workspace --older-than 7d

  Clean checkpoints with custom retention:
    newton checkpoints clean --workspace ./workspace --older-than 30d

CHECKPOINT STORAGE:
  Checkpoints are stored in .newton/checkpoints/ within your workspace.";

const ARTIFACTS_LONG_ABOUT: &str = "\
Artifacts provides tools to manage the output files, logs, and execution data \
generated during workflow execution.  Regular cleanup helps maintain good \
performance and disk space usage.

Subcommands:
  clean  Remove old workflow output files and execution artifacts to free disk space

EXAMPLES:
  Clean artifacts older than 7 days:
    newton artifacts clean --workspace ./workspace --older-than 7d

  Clean with custom retention period:
    newton artifacts clean --workspace ./workspace --older-than 30d

RETENTION FORMATS:
  Supported time formats for --older-than: 7d, 30d, 1w, 2w, 24h, 48h

ARTIFACT STORAGE:
  Artifacts are stored in .newton/artifacts/ within your workspace.";

const WEBHOOK_LONG_ABOUT: &str = "\
Webhook provides HTTP endpoints that can trigger workflow executions in \
response to external events.

This command enables integration with Git hosting services (GitHub, GitLab, \
Bitbucket), CI/CD platforms, monitoring systems, and custom applications.

Subcommands:
  serve   Start an HTTP server to receive webhook events and trigger workflows
  status  Display webhook endpoint configuration and server status

EXAMPLES:
  Start webhook server for a workflow:
    newton webhook serve workflow.yaml --workspace ./workspace

  Check webhook configuration status:
    newton webhook status workflow.yaml --workspace ./workspace

  Serve webhook with alternative file syntax:
    newton webhook serve --file ./workflows/deploy.yaml --workspace ./project

INTEGRATION:
  Configure your external services to send POST requests to the webhook URL.
  The webhook server will parse the incoming payload and trigger the workflow.

SECURITY:
  Webhook endpoints include built-in security features like request validation \
and rate limiting.  Configure authentication tokens and HTTPS for production deployments.";

const LOG_LONG_ABOUT: &str = "\
Log provides access to the per-task execution history stored in .newton/state/workflows/.

Use 'log list' to enumerate executions, and 'log show <execution-id>' to display
the resolved inputs, operator, and output for every task in that run.

Subcommands: list, show

DEFAULT LOG PATH:
    <workspace>/.newton/logs/newton.log

LOG LOCATION CONTROLS:
    --log-dir PATH         Override log directory (highest priority)
    logging.toml log_dir   Config file setting (second priority)
    workspace default      <workspace>/.newton/logs/ (fallback)

FILTER/VERBOSITY:
    RUST_LOG=debug         Sets tracing filter level only; does NOT change log directory.

EXAMPLES:
  List recent executions:
    newton log list --last 10
    newton log list --workspace ./workspace

  Show full execution log:
    newton log show <execution-id>
    newton log show <execution-id> --task <task-id> --verbose";

// ── shared helpers ────────────────────────────────────────────────────────────

/// Parse a comma-joined repeated-arg string (framework serialises `--arg k=v
/// --arg k2=v2` as `"k=v,k2=v2"`).  Values containing commas are not supported.
fn parse_kvp_list(s: &str) -> anyhow::Result<Vec<crate::cli::args::KeyValuePair>> {
    use std::str::FromStr;
    if s.is_empty() {
        return Ok(vec![]);
    }
    s.split(',')
        .map(|part| {
            crate::cli::args::KeyValuePair::from_str(part.trim())
                .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_002, e))
        })
        .collect()
}

fn get_bool(args: &CommandArgs, key: &str) -> bool {
    args.named.get(key).map(|s| s == "true").unwrap_or(false)
}

fn get_opt_path(args: &CommandArgs, key: &str) -> Option<PathBuf> {
    args.named.get(key).map(PathBuf::from)
}

fn get_opt_str(args: &CommandArgs, key: &str) -> Option<String> {
    args.named.get(key).cloned()
}

fn parse_output_format(args: &CommandArgs) -> OutputFormat {
    match args.named.get("format").map(String::as_str) {
        Some("json") => OutputFormat::Json,
        Some("prose") => OutputFormat::Prose,
        _ => OutputFormat::Text,
    }
}

// ── command constructors ──────────────────────────────────────────────────────

fn run_command() -> Command {
    Command {
        id: "run",
        summary: "Execute a workflow graph",
        syntax: Some("[WORKFLOW] [INPUT_FILE] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Execute a workflow graph",
            long_about: Some(RUN_LONG_ABOUT),
            examples: vec![
                "newton run workflow.yaml",
                "newton run workflow.yaml --workspace ./output --arg key=value",
                "newton run workflow.yaml --arg env=prod --arg version=1.2.3",
                "newton run workflow.yaml input.txt --workspace ./workspace --verbose",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "input-file",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Optional path written into triggers.payload.input_file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root directory",
                },
                ArgSpec {
                    name: "arg",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("arg"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into triggers.payload (repeatable)",
                },
                ArgSpec {
                    name: "set",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("set"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into workflow.context at runtime (repeatable)",
                },
                ArgSpec {
                    name: "trigger-json",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger-json"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Load JSON object as base trigger payload before --arg overrides",
                },
                ArgSpec {
                    name: "parallel-limit",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("parallel-limit"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Runtime override for bounded task concurrency",
                },
                ArgSpec {
                    name: "max-time-seconds",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("max-time-seconds"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Runtime wall-clock limit override (seconds)",
                },
                ArgSpec {
                    name: "verbose",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("verbose"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Print task stdout/stderr to terminal after each task completes",
                },
                ArgSpec {
                    name: "server",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("server"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Newton server URL to register this run (optional)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = RunArgs::try_from(args)?;
                commands::run(dto).await
            })
        }),
    }
}

fn init_command() -> Command {
    Command {
        id: "init",
        summary: "Initialize a Newton workspace with the default template",
        syntax: Some("[PATH] [OPTIONS]"),
        category: Some(categories::WORKSPACE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Initialize a Newton workspace with the default template",
            long_about: Some(INIT_LONG_ABOUT),
            examples: vec![
                "newton init .",
                "newton init ./workspace",
                "newton init . --template-source gonewton/newton-templates",
            ],
            args: vec![
                ArgSpec {
                    name: "path",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help:
                        "Directory where .newton/ will be created (defaults to current directory)",
                },
                ArgSpec {
                    name: "template-source",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("template-source"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Template source (GitHub repo, URL, or local path)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = InitArgs::try_from(args)?;
                init::run(dto)
            })
        }),
    }
}

fn batch_command() -> Command {
    Command {
        id: "batch",
        summary: "Process queued work items for a project",
        syntax: Some("<PROJECT_ID> [OPTIONS]"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Process queued work items for a project",
            long_about: Some(BATCH_LONG_ABOUT),
            examples: vec![
                "newton batch project-alpha",
                "newton batch project-alpha --workspace ./workspace",
                "newton batch project-alpha --once",
                "newton batch project-alpha --sleep 30",
            ],
            args: vec![
                ArgSpec {
                    name: "project-id",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Project identifier that maps to .newton/configs/<project_id>.conf",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root containing the .newton directory",
                },
                ArgSpec {
                    name: "once",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("once"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Process a single plan and exit instead of running as a daemon",
                },
                ArgSpec {
                    name: "sleep",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("sleep"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Sleep interval in seconds when the queue is empty (default: 60)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = BatchArgs::try_from(args)?;
                commands::batch(dto).await
            })
        }),
    }
}

fn serve_command() -> Command {
    Command {
        id: "serve",
        summary: "Start the Newton HTTP API server",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Start the Newton HTTP API server",
            long_about: Some(SERVE_LONG_ABOUT),
            examples: vec![
                "newton serve",
                "newton serve --host 0.0.0.0 --port 9000",
                "newton serve --host 0.0.0.0 --port 8080 &",
            ],
            args: vec![
                ArgSpec {
                    name: "host",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("host"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Host address to bind the server to (default: 127.0.0.1)",
                },
                ArgSpec {
                    name: "port",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("port"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Port to listen on (default: 8080)",
                },
                ArgSpec {
                    name: "ui-dir",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ui-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the built Newton UI dist directory (optional)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ServeArgs::try_from(args)?;
                commands::serve(dto).await.map_err(anyhow::Error::from)
            })
        }),
    }
}

fn monitor_command() -> Command {
    Command {
        id: "monitor",
        summary: "Monitor live ailoop channels via a terminal UI",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Monitor live ailoop channels via a terminal UI",
            long_about: Some(MONITOR_LONG_ABOUT),
            examples: vec![
                "newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080",
                "newton monitor",
                "newton monitor --http-url http://192.168.1.10:8081",
            ],
            args: vec![
                ArgSpec {
                    name: "http-url",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("http-url"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Override HTTP endpoint for the ailoop server",
                },
                ArgSpec {
                    name: "ws-url",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ws-url"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Override WebSocket endpoint for the ailoop server",
                },
                ArgSpec {
                    name: "backend",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("backend"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Also start the HTTP API backend server",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = MonitorArgs::try_from(args)?;
                commands::monitor(dto).await
            })
        }),
    }
}

fn validate_command() -> Command {
    Command {
        id: "validate",
        summary: "Validate a workflow graph definition",
        syntax: Some("[WORKFLOW] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Validate a workflow graph definition",
            long_about: Some(VALIDATE_LONG_ABOUT),
            examples: vec![
                "newton validate workflow.yaml",
                "newton validate --file ./workflows/my-workflow.yaml",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ValidateArgs::try_from(args)?;
                commands::validate(dto).map_err(anyhow::Error::from)
            })
        }),
    }
}

fn dot_command() -> Command {
    Command {
        id: "dot",
        summary: "Generate a visual diagram of the workflow graph",
        syntax: Some("[WORKFLOW] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Generate a visual diagram of the workflow graph",
            long_about: Some(DOT_LONG_ABOUT),
            examples: vec![
                "newton dot workflow.yaml",
                "newton dot workflow.yaml --out graph.dot",
                "newton dot workflow.yaml --out graph.dot && dot -Tpng graph.dot -o workflow.png",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
                ArgSpec {
                    name: "out",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("out"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Write DOT output to file instead of stdout",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = DotArgs::try_from(args)?;
                commands::dot(dto).map_err(anyhow::Error::from)
            })
        }),
    }
}

fn lint_command() -> Command {
    Command {
        id: "lint",
        summary: "Check workflow for best practices and potential issues",
        syntax: Some("[WORKFLOW] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Check workflow for best practices and potential issues",
            long_about: Some(LINT_LONG_ABOUT),
            examples: vec![
                "newton lint workflow.yaml",
                "newton lint workflow.yaml --format json",
                "newton lint --file ./workflows/production.yaml --format json",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("format"),
                    value_type: ArgValueType::Enum(vec!["text", "json", "prose"]),
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output format: text (default), json, prose",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = LintArgs::try_from(args)?;
                commands::lint(dto).map_err(anyhow::Error::from)
            })
        }),
    }
}

fn explain_command() -> Command {
    Command {
        id: "explain",
        summary: "Generate human-readable explanations of workflow behavior",
        syntax: Some("[WORKFLOW] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Generate human-readable explanations of workflow behavior",
            long_about: Some(EXPLAIN_LONG_ABOUT),
            examples: vec![
                "newton explain workflow.yaml --format text",
                "newton explain workflow.yaml --format prose",
                "newton explain workflow.yaml --arg env=production --format prose",
                "newton explain workflow.yaml --format json",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root directory",
                },
                ArgSpec {
                    name: "set",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("set"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into workflow.context at runtime (repeatable)",
                },
                ArgSpec {
                    name: "arg",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("arg"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Trigger payload override KEY=VALUE (repeatable)",
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("format"),
                    value_type: ArgValueType::Enum(vec!["text", "json", "prose"]),
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output format: text (default), json, prose",
                },
                ArgSpec {
                    name: "trigger-json",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger-json"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to JSON file containing manual trigger payload",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ExplainArgs::try_from(args)?;
                commands::explain(dto).map_err(anyhow::Error::from)
            })
        }),
    }
}

fn resume_command() -> Command {
    Command {
        id: "resume",
        summary: "Continue a workflow that was interrupted or stopped",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Continue a workflow that was interrupted or stopped",
            long_about: Some(RESUME_LONG_ABOUT),
            examples: vec![
                "newton resume --execution-id 12345678-1234-1234-1234-123456789abc",
                "newton resume --execution-id abcdef01-2345-6789-abcd-ef0123456789 --workspace ./project",
                "newton resume --execution-id 12345678-1234-1234-1234-123456789abc --allow-workflow-change",
            ],
            args: vec![
                ArgSpec {
                    name: "execution-id",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("execution-id"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "UUID of the workflow execution to resume",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root directory",
                },
                ArgSpec {
                    name: "allow-workflow-change",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("allow-workflow-change"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Allow resuming even if the workflow definition changed since checkpoint",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ResumeArgs::try_from(args)?;
                commands::resume(dto).await.map_err(anyhow::Error::from)
            })
        }),
    }
}

fn checkpoints_command() -> Command {
    Command {
        id: "checkpoints",
        summary: "Manage and inspect workflow execution checkpoints",
        syntax: Some("<list|clean> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Manage and inspect workflow execution checkpoints",
            long_about: Some(CHECKPOINTS_LONG_ABOUT),
            examples: vec![
                "newton checkpoints list --workspace ./workspace",
                "newton checkpoints list --workspace ./workspace --format-json",
                "newton checkpoints clean --workspace ./workspace --older-than 7d",
                "newton checkpoints clean --workspace ./workspace --older-than 30d",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec!["list", "clean"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: list or clean",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace path",
                },
                ArgSpec {
                    name: "format-json",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("format-json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output as JSON (list only)",
                },
                ArgSpec {
                    name: "older-than",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("older-than"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Duration threshold for clean (e.g. 7d, 1w, 24h); required for clean",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcmd = args
                    .named
                    .get("subcommand")
                    .map(String::as_str)
                    .unwrap_or("");
                match subcmd {
                    "list" => {
                        let dto = CheckpointsArgs {
                            command: CheckpointCommand::List {
                                workspace: get_opt_path(&args, "workspace"),
                                format_json: get_bool(&args, "format-json"),
                            },
                        };
                        commands::checkpoints(dto).map_err(anyhow::Error::from)
                    }
                    "clean" => {
                        let older_than =
                            args.named.get("older-than").cloned().ok_or_else(|| {
                                anyhow!(
                                    "{}: --older-than is required for checkpoints clean",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let dto = CheckpointsArgs {
                            command: CheckpointCommand::Clean {
                                workspace: get_opt_path(&args, "workspace"),
                                older_than,
                            },
                        };
                        commands::checkpoints(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown checkpoints subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
    }
}

fn artifacts_command() -> Command {
    Command {
        id: "artifacts",
        summary: "Manage workflow output files and execution artifacts",
        syntax: Some("<clean> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Manage workflow output files and execution artifacts",
            long_about: Some(ARTIFACTS_LONG_ABOUT),
            examples: vec![
                "newton artifacts clean --workspace ./workspace --older-than 7d",
                "newton artifacts clean --workspace ./workspace --older-than 30d",
                "newton artifacts clean --workspace /path/to/project --older-than 1w",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec!["clean"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: clean",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace path",
                },
                ArgSpec {
                    name: "older-than",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("older-than"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Duration threshold (e.g. 7d, 30d, 1w); required for clean",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcmd = args
                    .named
                    .get("subcommand")
                    .map(String::as_str)
                    .unwrap_or("");
                match subcmd {
                    "clean" => {
                        let older_than =
                            args.named.get("older-than").cloned().ok_or_else(|| {
                                anyhow!(
                                    "{}: --older-than is required for artifacts clean",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let dto = ArtifactsArgs {
                            command: ArtifactCommand::Clean {
                                workspace: get_opt_path(&args, "workspace"),
                                older_than,
                            },
                        };
                        commands::artifacts(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown artifacts subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
    }
}

fn webhook_command() -> Command {
    Command {
        id: "webhook",
        summary: "Run webhooks to trigger workflows from external events",
        syntax: Some("<serve|status> [WORKFLOW] --workspace <PATH>"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Run webhooks to trigger workflows from external events",
            long_about: Some(WEBHOOK_LONG_ABOUT),
            examples: vec![
                "newton webhook serve workflow.yaml --workspace ./workspace",
                "newton webhook status workflow.yaml --workspace ./workspace",
                "newton webhook serve --file ./workflows/deploy.yaml --workspace ./project",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec!["serve", "status"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: serve or status",
                },
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (alternative to positional; takes precedence)",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root directory (required)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcmd = args.named.get("subcommand").map(String::as_str).unwrap_or("").to_string();
                let workspace_str = args
                    .named
                    .get("workspace")
                    .cloned()
                    .ok_or_else(|| anyhow!("{}: --workspace is required for webhook {}", error_codes::CLI_MIG_002, subcmd))?;
                let workspace = PathBuf::from(workspace_str);
                let workflow_pos = get_opt_path(&args, "workflow");
                let file = get_opt_path(&args, "file");
                if let (Some(f), Some(p)) = (&file, &workflow_pos) {
                    if f != p {
                        return Err(anyhow!(
                            "{}: --file '{}' and positional workflow '{}' disagree; use one or the other",
                            error_codes::CLI_MIG_003, f.display(), p.display()
                        ));
                    }
                }
                match subcmd.as_str() {
                    "serve" => {
                        let dto = WebhookArgs {
                            command: WebhookCommand::Serve(WebhookServeArgs {
                                workflow_positional: workflow_pos,
                                file,
                                workspace,
                            }),
                        };
                        commands::webhook(dto).await.map_err(anyhow::Error::from)
                    }
                    "status" => {
                        let dto = WebhookArgs {
                            command: WebhookCommand::Status(WebhookStatusArgs {
                                workflow_positional: workflow_pos,
                                file,
                                workspace,
                            }),
                        };
                        commands::webhook(dto).await.map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!("{}: unknown webhook subcommand '{}'", error_codes::CLI_MIG_005, subcmd)),
                }
            })
        }),
    }
}

fn log_command() -> Command {
    Command {
        id: "log",
        summary: "List and replay workflow execution history",
        syntax: Some("<list|show> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "List and replay workflow execution history",
            long_about: Some(LOG_LONG_ABOUT),
            examples: vec![
                "newton log list --workspace ./workspace",
                "newton log list --last 10 --json",
                "newton log show <execution-id> --workspace ./workspace",
                "newton log show <execution-id> --task my-task --verbose",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec!["list", "show"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: list or show",
                },
                ArgSpec {
                    name: "execution-id",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Execution UUID (required for log show)",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace path",
                },
                ArgSpec {
                    name: "last",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("last"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Only list the N most recent executions (list only)",
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Emit machine-readable JSON",
                },
                ArgSpec {
                    name: "task",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("task"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Filter output to a single task ID (show only)",
                },
                ArgSpec {
                    name: "verbose",
                    kind: ArgKind::Flag,
                    short: Some('v'),
                    long: Some("verbose"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Expand single-task output for debugging (show only)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcmd = args
                    .named
                    .get("subcommand")
                    .map(String::as_str)
                    .unwrap_or("")
                    .to_string();
                match subcmd.as_str() {
                    "list" => {
                        let last = args
                            .named
                            .get("last")
                            .map(|s| {
                                let n: usize = s.parse().map_err(|_| {
                                    anyhow!(
                                        "{}: --last must be a positive integer",
                                        error_codes::CLI_MIG_002
                                    )
                                })?;
                                if n == 0 {
                                    return Err(anyhow!(
                                        "{}: --last must be a positive integer",
                                        error_codes::CLI_MIG_002
                                    ));
                                }
                                Ok(n)
                            })
                            .transpose()?;
                        let dto = LogArgs {
                            command: LogCommand::List {
                                workspace: get_opt_path(&args, "workspace"),
                                last,
                                json: get_bool(&args, "json"),
                            },
                        };
                        commands::log(dto).map_err(anyhow::Error::from)
                    }
                    "show" => {
                        let exec_id_str =
                            args.named.get("execution-id").cloned().ok_or_else(|| {
                                anyhow!(
                                    "{}: execution-id is required for log show",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let execution_id = Uuid::parse_str(&exec_id_str).map_err(|e| {
                            anyhow!(
                                "{}: invalid execution-id UUID: {}",
                                error_codes::CLI_MIG_002,
                                e
                            )
                        })?;
                        let dto = LogArgs {
                            command: LogCommand::Show {
                                execution_id,
                                workspace: get_opt_path(&args, "workspace"),
                                task: get_opt_str(&args, "task"),
                                verbose: get_bool(&args, "verbose"),
                                json: get_bool(&args, "json"),
                            },
                        };
                        commands::log(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown log subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
    }
}

// ── operational command builders (issue #231) ────────────────────────────────

fn health_command() -> Command {
    Command {
        id: "health",
        summary: "Print a one-line liveness status",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Print a one-line liveness status",
            long_about: Some(
                "Health prints `newton OK <version>` and exits 0 if the binary can run.\n\
                 No workspace, network, or config access — suitable for container probes.",
            ),
            examples: vec!["newton health"],
            args: vec![],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, _args| Box::pin(async move { ops::health::run() })),
    }
}

fn doctor_command() -> Command {
    Command {
        id: "doctor",
        summary: "Run local environment diagnostic probes",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Run local environment diagnostic probes",
            long_about: Some(
                "Doctor runs a small set of probes (workspace, config, ailoop reachability, gh,\n\
                 logging) and prints one `OK|FAIL|SKIP <name>: <detail>` line per probe.\n\
                 Exits 0 if all probes pass, 1 if any fail.",
            ),
            examples: vec!["newton doctor", "newton doctor --workspace ./workspace"],
            args: vec![ArgSpec {
                name: "workspace",
                kind: ArgKind::Option,
                short: None,
                long: Some("workspace"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Workspace root to probe (defaults to CWD with .newton/)",
            }],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let workspace = get_opt_path(&args, "workspace");
                let report = ops::doctor::run(ops::doctor::DoctorArgs { workspace })?;
                report.print();
                if report.any_failed() {
                    std::process::exit(1);
                }
                Ok(())
            })
        }),
    }
}

fn config_command() -> Command {
    Command {
        id: "config",
        summary: "Inspect resolved Newton configuration",
        syntax: Some("show [OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Inspect resolved Newton configuration",
            long_about: Some(
                "Config currently exposes one subcommand: `show`.\n\
                 `newton config show` prints the resolved configuration as JSON, with values\n\
                 whose key looks like a secret (token/secret/password/key) replaced by\n\
                 `***REDACTED***`.",
            ),
            examples: vec![
                "newton config show",
                "newton config show --workspace ./workspace",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: show (only supported value)",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root (optional)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let sub = args
                    .named
                    .get("subcommand")
                    .cloned()
                    .or_else(|| args.positional.first().cloned())
                    .unwrap_or_else(|| "show".to_string());
                if sub != "show" {
                    return Err(anyhow!(
                        "{}: only `config show` is supported (got `config {}`)",
                        error_codes::CLI_MIG_001,
                        sub
                    ));
                }
                let workspace = get_opt_path(&args, "workspace");
                ops::config_show::run(ops::config_show::ConfigShowArgs { workspace })
            })
        }),
    }
}

fn completion_command() -> Command {
    Command {
        id: "completion",
        summary: "Emit shell completion script",
        syntax: Some("<SHELL>"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Emit shell completion script",
            long_about: Some(
                "Completion writes a shell completion stub for the requested shell to stdout.\n\
                 Supported shells: bash, zsh, fish, powershell.",
            ),
            examples: vec![
                "newton completion bash",
                "newton completion zsh",
                "newton completion fish",
                "newton completion powershell",
            ],
            args: vec![ArgSpec {
                name: "shell",
                kind: ArgKind::Positional,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Target shell: bash | zsh | fish | powershell",
            }],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let shell_name = args
                    .named
                    .get("shell")
                    .cloned()
                    .or_else(|| args.positional.first().cloned())
                    .ok_or_else(|| {
                        anyhow!(
                            "{}: completion requires a shell argument",
                            error_codes::CLI_MIG_002
                        )
                    })?;
                let shell = ops::completion::Shell::from_str(&shell_name)?;
                ops::completion::run(shell)
            })
        }),
    }
}

#[cfg(feature = "ask")]
fn ask_command() -> Command {
    Command {
        id: "ask",
        summary: "Match a natural-language query to the closest command",
        syntax: Some("<QUERY>"),
        category: Some(categories::DIAGNOSTIC),
        spec: Some(Arc::new(CommandSpec {
            summary: "Match a natural-language query to the closest command",
            long_about: Some(
                "Ask ranks every registered command's summary/syntax/category against your\n\
                 query and prints the top 3 matches.  Substring + token-overlap scoring; no LLM.",
            ),
            examples: vec!["newton ask \"list checkpoints\""],
            args: vec![ArgSpec {
                name: "query",
                kind: ArgKind::Positional,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Free-text query (one positional arg)",
            }],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let query = args
                    .named
                    .get("query")
                    .cloned()
                    .unwrap_or_else(|| args.positional.join(" "));
                let summaries = ask_summaries();
                ask::run(&query, &summaries)
            })
        }),
    }
}

#[cfg(feature = "ask")]
fn ask_summaries() -> Vec<ask::CommandSummary> {
    fn s(name: &str, summary: &str, syntax: Option<&str>, category: &str) -> ask::CommandSummary {
        ask::CommandSummary {
            name: name.to_string(),
            summary: summary.to_string(),
            syntax: syntax.unwrap_or("").to_string(),
            category: category.to_string(),
        }
    }
    let cmds: Vec<Command> = vec![
        run_command(),
        init_command(),
        batch_command(),
        serve_command(),
        monitor_command(),
        validate_command(),
        dot_command(),
        lint_command(),
        explain_command(),
        resume_command(),
        checkpoints_command(),
        artifacts_command(),
        webhook_command(),
        log_command(),
        health_command(),
        doctor_command(),
        config_command(),
        completion_command(),
    ];
    cmds.into_iter()
        .map(|c| {
            s(
                c.id,
                c.summary,
                c.syntax,
                c.category.unwrap_or(categories::WORKFLOW),
            )
        })
        .collect()
}

// ── public entry point ────────────────────────────────────────────────────────

/// Build the Newton CLI application backed by `cli-framework`.
///
/// All 14 top-level commands are registered here.  Group commands
/// (checkpoints, artifacts, webhook, log) dispatch to their sub-actions
/// internally via the first positional arg.
pub fn build_app(ctx: NewtonContext) -> anyhow::Result<App<NewtonContext>> {
    #[allow(unused_mut)]
    let mut builder = AppBuilder::new()
        .with_version("newton", env!("CARGO_PKG_VERSION"))
        .register_command(run_command())?
        .register_command(init_command())?
        .register_command(batch_command())?
        .register_command(serve_command())?
        .register_command(monitor_command())?
        .register_command(validate_command())?
        .register_command(dot_command())?
        .register_command(lint_command())?
        .register_command(explain_command())?
        .register_command(resume_command())?
        .register_command(checkpoints_command())?
        .register_command(artifacts_command())?
        .register_command(webhook_command())?
        .register_command(log_command())?
        .register_command(health_command())?
        .register_command(doctor_command())?
        .register_command(config_command())?
        .register_command(completion_command())?;
    #[cfg(feature = "ask")]
    {
        builder = builder.register_command(ask_command())?;
    }
    builder
        .build(ctx)
        .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_001, e))
}

/// Stable list of command ids registered by [`build_app`].  Drives the
/// registry-uniqueness assertion (Goal 4.6) and ensures any rename surfaces
/// as a test failure rather than silent drift.
pub const REGISTERED_COMMAND_IDS: &[&str] = &[
    "run",
    "init",
    "batch",
    "serve",
    "monitor",
    "validate",
    "dot",
    "lint",
    "explain",
    "resume",
    "checkpoints",
    "artifacts",
    "webhook",
    "log",
    "health",
    "doctor",
    "config",
    "completion",
];

/// Returns every `Command` registered by `build_app`, in registration order.
/// Used by the metadata-audit unit test.
pub fn enumerate_commands() -> Vec<Command> {
    #[allow(unused_mut)]
    let mut cmds = vec![
        run_command(),
        init_command(),
        batch_command(),
        serve_command(),
        monitor_command(),
        validate_command(),
        dot_command(),
        lint_command(),
        explain_command(),
        resume_command(),
        checkpoints_command(),
        artifacts_command(),
        webhook_command(),
        log_command(),
        health_command(),
        doctor_command(),
        config_command(),
        completion_command(),
    ];
    #[cfg(feature = "ask")]
    {
        cmds.push(ask_command());
    }
    cmds
}

// ── TryFrom<CommandArgs> adapters ─────────────────────────────────────────────
//
// Each adapter converts the framework's flat CommandArgs (named: HashMap<String,String>)
// into the typed Newton DTO that the handler expects.

impl TryFrom<CommandArgs> for RunArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow_positional = get_opt_path(&args, "workflow");
        let input_file = get_opt_path(&args, "input-file");
        let file = get_opt_path(&args, "file");
        // CLI-MIG-003: both positional and --file supplied with different paths
        if let (Some(f), Some(p)) = (&file, &workflow_positional) {
            if f != p {
                return Err(anyhow!(
                    "{}: --file '{}' and positional workflow '{}' disagree",
                    error_codes::CLI_MIG_003,
                    f.display(),
                    p.display()
                ));
            }
        }
        let workspace = get_opt_path(&args, "workspace");
        let arg = parse_kvp_list(args.named.get("arg").map(String::as_str).unwrap_or(""))?;
        let set = parse_kvp_list(args.named.get("set").map(String::as_str).unwrap_or(""))?;
        let trigger_json = get_opt_path(&args, "trigger-json");
        let parallel_limit = args
            .named
            .get("parallel-limit")
            .map(|s| {
                s.parse::<usize>().map_err(|_| {
                    anyhow!(
                        "{}: --parallel-limit must be a positive integer",
                        error_codes::CLI_MIG_002
                    )
                })
            })
            .transpose()?;
        let max_time_seconds = args
            .named
            .get("max-time-seconds")
            .map(|s| {
                s.parse::<u64>().map_err(|_| {
                    anyhow!(
                        "{}: --max-time-seconds must be a non-negative integer",
                        error_codes::CLI_MIG_002
                    )
                })
            })
            .transpose()?;
        let verbose = get_bool(&args, "verbose");
        let server = get_opt_str(&args, "server");
        Ok(RunArgs {
            workflow_positional,
            input_file,
            file,
            workspace,
            arg,
            set,
            trigger_json,
            parallel_limit,
            max_time_seconds,
            verbose,
            server,
        })
    }
}

impl TryFrom<CommandArgs> for InitArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        Ok(InitArgs {
            path: get_opt_path(&args, "path"),
            template_source: get_opt_str(&args, "template-source"),
        })
    }
}

impl TryFrom<CommandArgs> for BatchArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let project_id = args
            .named
            .get("project-id")
            .cloned()
            .ok_or_else(|| anyhow!("{}: project-id is required", error_codes::CLI_MIG_002))?;
        let sleep = args
            .named
            .get("sleep")
            .map(|s| {
                s.parse::<u64>().map_err(|_| {
                    anyhow!(
                        "{}: --sleep must be a non-negative integer",
                        error_codes::CLI_MIG_002
                    )
                })
            })
            .transpose()?
            .unwrap_or(60);
        Ok(BatchArgs {
            project_id,
            workspace: get_opt_path(&args, "workspace"),
            once: get_bool(&args, "once"),
            sleep,
        })
    }
}

impl TryFrom<CommandArgs> for ServeArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let host = args
            .named
            .get("host")
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = args
            .named
            .get("port")
            .map(|s| {
                s.parse::<i64>()
                    .map_err(|_| anyhow!("{}: --port must be an integer", error_codes::CLI_MIG_002))
                    .and_then(|n| {
                        u16::try_from(n).map_err(|_| {
                            anyhow!(
                                "{}: --port must be in range 0-65535",
                                error_codes::CLI_MIG_002
                            )
                        })
                    })
            })
            .transpose()?
            .unwrap_or(8080);
        Ok(ServeArgs {
            host,
            port,
            ui_dir: get_opt_path(&args, "ui-dir"),
        })
    }
}

impl TryFrom<CommandArgs> for MonitorArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        Ok(MonitorArgs {
            http_url: get_opt_str(&args, "http-url"),
            ws_url: get_opt_str(&args, "ws-url"),
            backend: get_bool(&args, "backend"),
        })
    }
}

impl TryFrom<CommandArgs> for ValidateArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow_positional = get_opt_path(&args, "workflow");
        let file = get_opt_path(&args, "file");
        if let (Some(f), Some(p)) = (&file, &workflow_positional) {
            if f != p {
                return Err(anyhow!(
                    "{}: --file '{}' and positional workflow '{}' disagree",
                    error_codes::CLI_MIG_003,
                    f.display(),
                    p.display()
                ));
            }
        }
        Ok(ValidateArgs {
            workflow_positional,
            file,
        })
    }
}

impl TryFrom<CommandArgs> for DotArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow_positional = get_opt_path(&args, "workflow");
        let file = get_opt_path(&args, "file");
        if let (Some(f), Some(p)) = (&file, &workflow_positional) {
            if f != p {
                return Err(anyhow!(
                    "{}: --file '{}' and positional workflow '{}' disagree",
                    error_codes::CLI_MIG_003,
                    f.display(),
                    p.display()
                ));
            }
        }
        Ok(DotArgs {
            workflow_positional,
            file,
            out: get_opt_path(&args, "out"),
        })
    }
}

impl TryFrom<CommandArgs> for LintArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow_positional = get_opt_path(&args, "workflow");
        let file = get_opt_path(&args, "file");
        if let (Some(f), Some(p)) = (&file, &workflow_positional) {
            if f != p {
                return Err(anyhow!(
                    "{}: --file '{}' and positional workflow '{}' disagree",
                    error_codes::CLI_MIG_003,
                    f.display(),
                    p.display()
                ));
            }
        }
        Ok(LintArgs {
            workflow_positional,
            file,
            format: parse_output_format(&args),
        })
    }
}

impl TryFrom<CommandArgs> for ExplainArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow_positional = get_opt_path(&args, "workflow");
        let file = get_opt_path(&args, "file");
        if let (Some(f), Some(p)) = (&file, &workflow_positional) {
            if f != p {
                return Err(anyhow!(
                    "{}: --file '{}' and positional workflow '{}' disagree",
                    error_codes::CLI_MIG_003,
                    f.display(),
                    p.display()
                ));
            }
        }
        let set = parse_kvp_list(args.named.get("set").map(String::as_str).unwrap_or(""))?;
        let arg = parse_kvp_list(args.named.get("arg").map(String::as_str).unwrap_or(""))?;
        Ok(ExplainArgs {
            workflow_positional,
            file,
            workspace: get_opt_path(&args, "workspace"),
            set,
            arg,
            format: parse_output_format(&args),
            trigger_json: get_opt_path(&args, "trigger-json"),
        })
    }
}

impl TryFrom<CommandArgs> for ResumeArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let execution_id = args
            .named
            .get("execution-id")
            .ok_or_else(|| anyhow!("{}: --execution-id is required", error_codes::CLI_MIG_002))
            .and_then(|s| {
                Uuid::parse_str(s).map_err(|e| {
                    anyhow!(
                        "{}: --execution-id must be a valid UUID: {}",
                        error_codes::CLI_MIG_002,
                        e
                    )
                })
            })?;
        Ok(ResumeArgs {
            execution_id,
            workspace: get_opt_path(&args, "workspace"),
            allow_workflow_change: get_bool(&args, "allow-workflow-change"),
        })
    }
}

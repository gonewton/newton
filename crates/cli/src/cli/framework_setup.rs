//! cli-framework registration for Newton CLI (spec 273 surface).
//!
//! All Command / CommandSpec / ArgSpec declarations and the `build_app()`
//! entry point used by `crates/cli/src/main.rs` live here.
//!
//! ## Nested-command note
//! The framework routes via its root-level `commands` map; nested paths in
//! `tree_commands` are not yet dispatched by the clap adapter.  Group
//! commands (workflow, runs, checkpoint, artifact, webhook) are therefore
//! registered at root level and dispatch internally via their first
//! positional arg (subcommand).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::app::{App, AppBuilder};
use cli_framework::command::{Command, CommandArgs};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use uuid::Uuid;

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, BatchArgs, CheckpointArgs, CheckpointCommand, DotArgs,
    ExplainArgs, GraphFormat, InitArgs, LintArgs, MonitorArgs, OutputFormat, ResumeArgs, RunArgs,
    RunsArgs, RunsCommand, ServeArgs, ValidateArgs, WebhookArgs, WebhookCommand, WebhookServeArgs,
    WebhookStatusArgs,
};
use crate::cli::categories;
use crate::cli::context::NewtonContext;
use crate::cli::ops;
use crate::cli::{commands, init};

#[cfg(feature = "ask")]
use crate::cli::ask;

/// Stable error codes for the migration adapter layer.
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
    /// `serve --with-mcp`: invalid `--mcp-path` value (issue #294).
    pub const NEWTON_SERVE_MCP_001: &str = "NEWTON-SERVE-MCP-001";
    /// `serve --with-mcp`: `--mcp-path` collides with an existing Newton REST route prefix (issue #294).
    pub const NEWTON_SERVE_MCP_002: &str = "NEWTON-SERVE-MCP-002";
    /// `serve --with-mcp`: cli-framework MCP-mount API unavailable in linked version (issue #294).
    pub const NEWTON_SERVE_MCP_003: &str = "NEWTON-SERVE-MCP-003";
    /// `serve --with-mcp`: cli-framework returned an error while constructing the MCP router (issue #294).
    pub const NEWTON_SERVE_MCP_004: &str = "NEWTON-SERVE-MCP-004";
}

// ── help-text constants ───────────────────────────────────────────────────────

const RUN_LONG_ABOUT: &str = "\
Run executes a workflow graph defined in YAML, with optional trigger payload.

EXAMPLES:
  Basic workflow execution:
    newton run workflow.yaml

  With workspace and trigger data:
    newton run workflow.yaml --workspace ./output --trigger key=value

  Multiple trigger arguments:
    newton run workflow.yaml --trigger env=prod --trigger version=1.2.3

  With input file and verbose output:
    newton run workflow.yaml input.txt --workspace ./workspace --verbose

  With base trigger payload from a JSON file:
    newton run workflow.yaml --trigger-file payload.json --trigger override=1";

const INIT_LONG_ABOUT: &str = "\
Init creates the .newton workspace layout, installs the Newton template with \
aikit-sdk, and writes default configs so you can run immediately.

EXAMPLES:
  Initialize current directory:
    newton init .

  Initialize a specific directory:
    newton init ./workspace

  Initialize with custom template source:
    newton init . --template gonewton/newton-templates";

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

  Custom poll interval (seconds):
    newton batch project-alpha --poll-interval 30";

const SERVE_LONG_ABOUT: &str = "\
Serve runs the Newton HTTP/WebSocket API for UIs, agents, and integrations.
Full REST contract: openapi/newton-backend-parity.yaml.

EXAMPLES:
  Start API server on default port:
    newton serve

  Start on custom host and port:
    newton serve --host 0.0.0.0 --port 9000

  Serve a built UI from a static directory:
    newton serve --static-ui ./ui/dist";

const MONITOR_LONG_ABOUT: &str = "\
Monitor listens to every project/branch channel from the workspace using a \
WebSocket/HTTP mix and lets you answer questions or approve authorizations in a queue.

EXAMPLES:
  Using both CLI overrides:
    newton monitor --ailoop-http http://127.0.0.1:8080 --ailoop-ws ws://127.0.0.1:8080

  Using .newton/configs/monitor.conf:
    newton monitor

  Also start the Newton HTTP API alongside the monitor:
    newton monitor --with-api";

const WORKFLOW_LONG_ABOUT: &str = "\
Workflow groups commands that operate on a workflow YAML file: validate, \
lint, preview, and graph.

Subcommands:
  validate <FILE>    Validate a workflow graph definition
  lint <FILE>        Check workflow for best practices and issues
  preview <FILE>     Preview what running the workflow would do
  graph <FILE>       Render the workflow graph (default --format dot)

EXAMPLES:
  newton workflow validate workflow.yaml
  newton workflow lint workflow.yaml --format json
  newton workflow preview workflow.yaml --trigger env=prod --format prose
  newton workflow graph workflow.yaml --output graph.dot";

const RESUME_LONG_ABOUT: &str = "\
Resume restarts a workflow execution from its last saved checkpoint.

EXAMPLES:
  Resume a specific run:
    newton resume --run-id 12345678-1234-1234-1234-123456789abc

  Resume with custom workspace:
    newton resume --run-id abcdef01-2345-6789-abcd-ef0123456789 --workspace ./project

  Resume and allow workflow definition changes:
    newton resume --run-id 12345678-1234-1234-1234-123456789abc --allow-workflow-change

FINDING RUN IDs:
  newton checkpoint list --workspace ./workspace";

const CHECKPOINT_LONG_ABOUT: &str = "\
Checkpoint provides tools to manage the saved states that allow workflow \
resumption after interruption.

Subcommands:
  list   Display available workflow executions and their checkpoint details
  clean  Remove old checkpoint files to free up disk space

EXAMPLES:
  newton checkpoint list --workspace ./workspace
  newton checkpoint list --workspace ./workspace --json
  newton checkpoint clean --workspace ./workspace --older-than 7d";

const ARTIFACT_LONG_ABOUT: &str = "\
Artifact provides tools to manage the output files, logs, and execution data \
generated during workflow execution.

Subcommands:
  clean  Remove old workflow output files and execution artifacts

EXAMPLES:
  newton artifact clean --workspace ./workspace --older-than 7d
  newton artifact clean --workspace ./workspace --older-than 30d";

const WEBHOOK_LONG_ABOUT: &str = "\
Webhook provides HTTP endpoints that can trigger workflow executions in \
response to external events.

Subcommands:
  serve   Start an HTTP server to receive webhook events
  status  Display webhook endpoint configuration and status

EXAMPLES:
  newton webhook serve --workflow workflow.yaml --workspace ./workspace
  newton webhook status --workflow workflow.yaml --workspace ./workspace";

const RUNS_LONG_ABOUT: &str = "\
Runs provides access to the per-task execution history stored in .newton/state/workflows/.

Subcommands:
  list   Enumerate workflow execution history
  show   Replay task-by-task detail for a specific run

EXAMPLES:
  newton runs list --last 10
  newton runs list --workspace ./workspace
  newton runs show <run-id>
  newton runs show <run-id> --task <task-id> --verbose";

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

fn require_workflow_path(args: &CommandArgs, label: &str) -> anyhow::Result<PathBuf> {
    get_opt_path(args, "workflow").ok_or_else(|| {
        anyhow!(
            "{}: workflow file is required for {}",
            error_codes::CLI_MIG_002,
            label
        )
    })
}

// ── command constructors ──────────────────────────────────────────────────────

fn run_command() -> Command {
    Command {
        id: "run",
        summary: "Execute a workflow graph",
        syntax: Some("<WORKFLOW> [INPUT_FILE] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Execute a workflow graph",
            long_about: Some(RUN_LONG_ABOUT),
            examples: vec![
                "newton run workflow.yaml",
                "newton run workflow.yaml --workspace ./output --trigger key=value",
                "newton run workflow.yaml --trigger env=prod --trigger version=1.2.3",
                "newton run workflow.yaml input.txt --workspace ./workspace --verbose",
            ],
            args: vec![
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (required)",
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
                    name: "trigger",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into trigger payload (repeatable; VALUE may be @path)",
                },
                ArgSpec {
                    name: "context",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("context"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into workflow.context at runtime (repeatable)",
                },
                ArgSpec {
                    name: "trigger-file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger-file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Load JSON object as base trigger payload before --trigger overrides",
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
                    name: "timeout",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("timeout"),
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
                    short: Some('v'),
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
        expose_mcp: false,
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
                "newton init . --template gonewton/newton-templates",
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
                    name: "template",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("template"),
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
        expose_mcp: false,
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
                "newton batch project-alpha --poll-interval 30",
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
                    name: "poll-interval",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("poll-interval"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Seconds to wait when the queue is empty (default: 60)",
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
        expose_mcp: false,
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
                "newton serve --static-ui ./ui/dist",
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
                    name: "static-ui",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("static-ui"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the built Newton UI dist directory (optional)",
                },
                ArgSpec {
                    name: "with-mcp",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("with-mcp"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Mount the MCP HTTP router on the same listener as the Newton API",
                },
                ArgSpec {
                    name: "mcp-path",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("mcp-path"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path prefix where the MCP HTTP router is mounted (default: /mcp)",
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
        expose_mcp: false,
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
                "newton monitor --ailoop-http http://127.0.0.1:8080 --ailoop-ws ws://127.0.0.1:8080",
                "newton monitor",
                "newton monitor --with-api",
            ],
            args: vec![
                ArgSpec {
                    name: "ailoop-http",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ailoop-http"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "HTTP endpoint for the ailoop server",
                },
                ArgSpec {
                    name: "ailoop-ws",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ailoop-ws"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "WebSocket endpoint for the ailoop server",
                },
                ArgSpec {
                    name: "with-api",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("with-api"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Also start the Newton HTTP API alongside the monitor",
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
        expose_mcp: false,
    }
}

fn workflow_command() -> Command {
    Command {
        id: "workflow",
        summary: "Operate on a workflow YAML file (validate/lint/preview/graph)",
        syntax: Some("<validate|lint|preview|graph> <FILE> [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Operate on a workflow YAML file (validate/lint/preview/graph)",
            long_about: Some(WORKFLOW_LONG_ABOUT),
            examples: vec![
                "newton workflow validate workflow.yaml",
                "newton workflow lint workflow.yaml --format json",
                "newton workflow preview workflow.yaml --trigger env=prod --format prose",
                "newton workflow graph workflow.yaml --output graph.dot",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec!["validate", "lint", "preview", "graph"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: validate | lint | preview | graph",
                },
                ArgSpec {
                    name: "workflow",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file",
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output format (lint: text|json; preview: text|json|prose; graph: dot)",
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
                    help: "Workspace root directory (preview)",
                },
                ArgSpec {
                    name: "context",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("context"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Merge KEY=VALUE into workflow.context at runtime (preview)",
                },
                ArgSpec {
                    name: "trigger",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Trigger payload override KEY=VALUE (preview)",
                },
                ArgSpec {
                    name: "trigger-file",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("trigger-file"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "JSON file with base trigger payload (preview)",
                },
                ArgSpec {
                    name: "output",
                    kind: ArgKind::Option,
                    short: Some('o'),
                    long: Some("output"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output destination file (graph)",
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
                    "validate" => {
                        let workflow = require_workflow_path(&args, "workflow validate")?;
                        commands::validate(ValidateArgs { workflow }).map_err(anyhow::Error::from)
                    }
                    "lint" => {
                        let workflow = require_workflow_path(&args, "workflow lint")?;
                        commands::lint(LintArgs {
                            workflow,
                            format: parse_output_format(&args),
                        })
                        .map_err(anyhow::Error::from)
                    }
                    "preview" => {
                        let workflow = require_workflow_path(&args, "workflow preview")?;
                        let context = parse_kvp_list(
                            args.named.get("context").map(String::as_str).unwrap_or(""),
                        )?;
                        let trigger = parse_kvp_list(
                            args.named.get("trigger").map(String::as_str).unwrap_or(""),
                        )?;
                        commands::explain(ExplainArgs {
                            workflow,
                            workspace: get_opt_path(&args, "workspace"),
                            context,
                            trigger,
                            format: parse_output_format(&args),
                            trigger_file: get_opt_path(&args, "trigger-file"),
                        })
                        .map_err(anyhow::Error::from)
                    }
                    "graph" => {
                        let workflow = require_workflow_path(&args, "workflow graph")?;
                        let format = match args.named.get("format").map(String::as_str) {
                            Some("dot") | None => GraphFormat::Dot,
                            Some(other) => {
                                return Err(anyhow!(
                                    "{}: unknown graph format '{}' (supported: dot)",
                                    error_codes::CLI_MIG_002,
                                    other
                                ))
                            }
                        };
                        commands::dot(DotArgs {
                            workflow,
                            format,
                            output: get_opt_path(&args, "output"),
                        })
                        .map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown workflow subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: false,
    }
}

fn resume_command() -> Command {
    Command {
        id: "resume",
        summary: "Continue a workflow that was interrupted or stopped",
        syntax: Some("--run-id <UUID> [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Continue a workflow that was interrupted or stopped",
            long_about: Some(RESUME_LONG_ABOUT),
            examples: vec![
                "newton resume --run-id 12345678-1234-1234-1234-123456789abc",
                "newton resume --run-id abcdef01-2345-6789-abcd-ef0123456789 --workspace ./project",
                "newton resume --run-id 12345678-1234-1234-1234-123456789abc --allow-workflow-change",
            ],
            args: vec![
                ArgSpec {
                    name: "run-id",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("run-id"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "UUID of the workflow run to resume",
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
        expose_mcp: false,
    }
}

fn checkpoint_command() -> Command {
    Command {
        id: "checkpoint",
        summary: "Manage and inspect workflow execution checkpoints",
        syntax: Some("<list|clean> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Manage and inspect workflow execution checkpoints",
            long_about: Some(CHECKPOINT_LONG_ABOUT),
            examples: vec![
                "newton checkpoint list --workspace ./workspace",
                "newton checkpoint list --workspace ./workspace --json",
                "newton checkpoint clean --workspace ./workspace --older-than 7d",
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
                    name: "json",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("json"),
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
                        let dto = CheckpointArgs {
                            command: CheckpointCommand::List {
                                workspace: get_opt_path(&args, "workspace"),
                                json: get_bool(&args, "json"),
                            },
                        };
                        commands::checkpoints(dto).map_err(anyhow::Error::from)
                    }
                    "clean" => {
                        let older_than =
                            args.named.get("older-than").cloned().ok_or_else(|| {
                                anyhow!(
                                    "{}: --older-than is required for checkpoint clean",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let dto = CheckpointArgs {
                            command: CheckpointCommand::Clean {
                                workspace: get_opt_path(&args, "workspace"),
                                older_than,
                            },
                        };
                        commands::checkpoints(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown checkpoint subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: false,
    }
}

fn artifact_command() -> Command {
    Command {
        id: "artifact",
        summary: "Manage workflow output files and execution artifacts",
        syntax: Some("<clean> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Manage workflow output files and execution artifacts",
            long_about: Some(ARTIFACT_LONG_ABOUT),
            examples: vec![
                "newton artifact clean --workspace ./workspace --older-than 7d",
                "newton artifact clean --workspace ./workspace --older-than 30d",
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
                                    "{}: --older-than is required for artifact clean",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let dto = ArtifactArgs {
                            command: ArtifactCommand::Clean {
                                workspace: get_opt_path(&args, "workspace"),
                                older_than,
                            },
                        };
                        commands::artifacts(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown artifact subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: false,
    }
}

fn webhook_command() -> Command {
    Command {
        id: "webhook",
        summary: "Run webhooks to trigger workflows from external events",
        syntax: Some("<serve|status> --workflow <PATH> --workspace <PATH>"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Run webhooks to trigger workflows from external events",
            long_about: Some(WEBHOOK_LONG_ABOUT),
            examples: vec![
                "newton webhook serve --workflow workflow.yaml --workspace ./workspace",
                "newton webhook status --workflow workflow.yaml --workspace ./workspace",
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
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workflow"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the workflow YAML file (required for serve)",
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
                let subcmd = args
                    .named
                    .get("subcommand")
                    .map(String::as_str)
                    .unwrap_or("")
                    .to_string();
                let workspace_str = args.named.get("workspace").cloned().ok_or_else(|| {
                    anyhow!(
                        "{}: --workspace is required for webhook {}",
                        error_codes::CLI_MIG_002,
                        subcmd
                    )
                })?;
                let workspace = PathBuf::from(workspace_str);
                let workflow = get_opt_path(&args, "workflow");
                match subcmd.as_str() {
                    "serve" => {
                        let workflow = workflow.ok_or_else(|| {
                            anyhow!(
                                "{}: --workflow is required for webhook serve",
                                error_codes::CLI_MIG_002
                            )
                        })?;
                        let dto = WebhookArgs {
                            command: WebhookCommand::Serve(WebhookServeArgs {
                                workflow,
                                workspace,
                            }),
                        };
                        commands::webhook(dto).await.map_err(anyhow::Error::from)
                    }
                    "status" => {
                        let dto = WebhookArgs {
                            command: WebhookCommand::Status(WebhookStatusArgs {
                                workflow,
                                workspace,
                            }),
                        };
                        commands::webhook(dto).await.map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown webhook subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: false,
    }
}

fn runs_command() -> Command {
    Command {
        id: "runs",
        summary: "List and replay workflow execution history",
        syntax: Some("<list|show> [OPTIONS]"),
        category: Some(categories::MAINTENANCE),
        spec: Some(Arc::new(CommandSpec {
            summary: "List and replay workflow execution history",
            long_about: Some(RUNS_LONG_ABOUT),
            examples: vec![
                "newton runs list --workspace ./workspace",
                "newton runs list --last 10 --json",
                "newton runs show <run-id> --workspace ./workspace",
                "newton runs show <run-id> --task my-task --verbose",
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
                    name: "run-id",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Run UUID (required for `runs show`)",
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
                                    anyhow!("LOG-003: runs list --last must be a positive integer")
                                })?;
                                if n == 0 {
                                    return Err(anyhow!(
                                        "LOG-003: runs list --last must be a positive integer"
                                    ));
                                }
                                Ok(n)
                            })
                            .transpose()?;
                        let dto = RunsArgs {
                            command: RunsCommand::List {
                                workspace: get_opt_path(&args, "workspace"),
                                last,
                                json: get_bool(&args, "json"),
                            },
                        };
                        commands::log(dto).map_err(anyhow::Error::from)
                    }
                    "show" => {
                        let run_id_str = args.named.get("run-id").cloned().ok_or_else(|| {
                            anyhow!(
                                "{}: <RUN_ID> is required for `runs show`",
                                error_codes::CLI_MIG_002
                            )
                        })?;
                        let run_id = Uuid::parse_str(&run_id_str).map_err(|e| {
                            anyhow!("{}: invalid run-id UUID: {}", error_codes::CLI_MIG_002, e)
                        })?;
                        let dto = RunsArgs {
                            command: RunsCommand::Show {
                                run_id,
                                workspace: get_opt_path(&args, "workspace"),
                                task: get_opt_str(&args, "task"),
                                verbose: get_bool(&args, "verbose"),
                                json: get_bool(&args, "json"),
                            },
                        };
                        commands::log(dto).map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown runs subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: false,
    }
}

// ── operational command builders ──────────────────────────────────────────────

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
        expose_mcp: false,
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
        expose_mcp: false,
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
        expose_mcp: false,
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
        expose_mcp: false,
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
        expose_mcp: false,
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
        workflow_command(),
        resume_command(),
        checkpoint_command(),
        artifact_command(),
        webhook_command(),
        runs_command(),
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

/// Build an `axum::Router` that mounts the cli-framework MCP HTTP transport
/// under `mcp_path` on the caller-owned listener (issue #294).
///
/// This is the Newton-side adapter for what `aroff/cli-framework#29` aims to
/// expose upstream. Until the upstream `App::into_mcp_router(...)` lands,
/// Newton constructs the equivalent registry/service/router stack directly
/// from cli-framework's public MCP primitives. If the upstream API later
/// becomes available the implementation switches to that without changing the
/// public function signature here.
///
/// Returns:
/// * `NEWTON-SERVE-MCP-003` — required upstream MCP-mount API not available
///   in the linked cli-framework version.
/// * `NEWTON-SERVE-MCP-004` — cli-framework returned an error while building
///   the registry, the tool registry, or the HTTP service.
pub fn build_mcp_router_for_serve(
    _ctx: NewtonContext,
    mcp_path: &str,
) -> anyhow::Result<axum::Router> {
    use cli_framework::command::registry::CommandRegistry;
    use cli_framework::mcp::{CliFrameworkHandler, McpToolRegistry};
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    // Build the same flat registry that `build_app` populates so MCP tool
    // names line up exactly with Newton's `--mcp-serve` mode.
    let mut registry = CommandRegistry::new();
    for cmd in enumerate_commands() {
        registry.register(cmd);
    }

    let tool_registry =
        std::sync::Arc::new(McpToolRegistry::from_command_registry(&registry, "newton"));
    if tool_registry.tool_count() == 0 {
        return Err(anyhow!(
            "{}: cli-framework returned an empty MCP tool registry",
            error_codes::NEWTON_SERVE_MCP_004
        ));
    }

    let session_manager = std::sync::Arc::new(LocalSessionManager::default());
    let config = StreamableHttpServerConfig::default();
    let service = StreamableHttpService::new(
        {
            let tool_registry = std::sync::Arc::clone(&tool_registry);
            move || {
                Ok(CliFrameworkHandler::new(std::sync::Arc::clone(
                    &tool_registry,
                )))
            }
        },
        session_manager,
        config,
    );

    Ok(axum::Router::new().nest_service(mcp_path, service))
}

/// Build the Newton CLI application backed by `cli-framework`.
pub fn build_app(ctx: NewtonContext) -> anyhow::Result<App<NewtonContext>> {
    #[allow(unused_mut)]
    let mut builder = AppBuilder::new()
        .with_version("newton", env!("CARGO_PKG_VERSION"))
        .register_command(run_command())?
        .register_command(init_command())?
        .register_command(batch_command())?
        .register_command(serve_command())?
        .register_command(monitor_command())?
        .register_command(workflow_command())?
        .register_command(resume_command())?
        .register_command(checkpoint_command())?
        .register_command(artifact_command())?
        .register_command(webhook_command())?
        .register_command(runs_command())?
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

/// Stable list of command ids registered by [`build_app`].
pub const REGISTERED_COMMAND_IDS: &[&str] = &[
    "run",
    "init",
    "batch",
    "serve",
    "monitor",
    "workflow",
    "resume",
    "checkpoint",
    "artifact",
    "webhook",
    "runs",
    "health",
    "doctor",
    "config",
    "completion",
];

/// Returns every `Command` registered by `build_app`, in registration order.
pub fn enumerate_commands() -> Vec<Command> {
    #[allow(unused_mut)]
    let mut cmds = vec![
        run_command(),
        init_command(),
        batch_command(),
        serve_command(),
        monitor_command(),
        workflow_command(),
        resume_command(),
        checkpoint_command(),
        artifact_command(),
        webhook_command(),
        runs_command(),
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

impl TryFrom<CommandArgs> for RunArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let workflow = require_workflow_path(&args, "run")?;
        let input_file = get_opt_path(&args, "input-file");
        let workspace = get_opt_path(&args, "workspace");
        let trigger = parse_kvp_list(args.named.get("trigger").map(String::as_str).unwrap_or(""))?;
        let context = parse_kvp_list(args.named.get("context").map(String::as_str).unwrap_or(""))?;
        let trigger_file = get_opt_path(&args, "trigger-file");
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
        let timeout_seconds = args
            .named
            .get("timeout")
            .map(|s| {
                s.parse::<u64>().map_err(|_| {
                    anyhow!(
                        "{}: --timeout must be a non-negative integer",
                        error_codes::CLI_MIG_002
                    )
                })
            })
            .transpose()?;
        let verbose = get_bool(&args, "verbose");
        let server = get_opt_str(&args, "server");
        Ok(RunArgs {
            workflow,
            input_file,
            workspace,
            trigger,
            context,
            trigger_file,
            parallel_limit,
            timeout_seconds,
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
            template: get_opt_str(&args, "template"),
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
        let poll_interval_seconds = args
            .named
            .get("poll-interval")
            .map(|s| {
                s.parse::<u64>().map_err(|_| {
                    anyhow!(
                        "{}: --poll-interval must be a non-negative integer",
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
            poll_interval_seconds,
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
        let with_mcp = get_bool(&args, "with-mcp");
        let mcp_path = args
            .named
            .get("mcp-path")
            .cloned()
            .unwrap_or_else(|| "/mcp".to_string());
        Ok(ServeArgs {
            host,
            port,
            static_ui: get_opt_path(&args, "static-ui"),
            with_mcp,
            mcp_path,
        })
    }
}

impl TryFrom<CommandArgs> for MonitorArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        Ok(MonitorArgs {
            ailoop_http: get_opt_str(&args, "ailoop-http"),
            ailoop_ws: get_opt_str(&args, "ailoop-ws"),
            with_api: get_bool(&args, "with-api"),
        })
    }
}

impl TryFrom<CommandArgs> for ResumeArgs {
    type Error = anyhow::Error;

    fn try_from(args: CommandArgs) -> Result<Self, Self::Error> {
        let run_id = args
            .named
            .get("run-id")
            .ok_or_else(|| anyhow!("{}: --run-id is required", error_codes::CLI_MIG_002))
            .and_then(|s| {
                Uuid::parse_str(s).map_err(|e| {
                    anyhow!(
                        "{}: --run-id must be a valid UUID: {}",
                        error_codes::CLI_MIG_002,
                        e
                    )
                })
            })?;
        Ok(ResumeArgs {
            run_id,
            workspace: get_opt_path(&args, "workspace"),
            allow_workflow_change: get_bool(&args, "allow-workflow-change"),
        })
    }
}

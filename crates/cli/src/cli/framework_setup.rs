//! cli-framework registration for Newton CLI (spec 273 surface).
//!
//! All Command / CommandSpec / ArgSpec declarations and the `build_app()`
//! entry point used by `crates/cli/src/main.rs` live here.
//!
//! ## MCP tool surface (issue #309)
//!
//! Newton uses `McpToolExportPolicy::ExposeMcpOnly` for both MCP entry points
//! (`newton --mcp-serve` and `newton serve --with-mcp`). Only commands with
//! `expose_mcp: true` appear in `tools/list`. The curated allowlist is:
//! `config`, `health`, `workflow`.
//! All other commands (`init`, `batch`, `serve`, `checkpoint`, `artifact`,
//! `webhook`, `doctor`, `completion`, `ask`) are excluded from the MCP surface.
//! `resume` and `runs` are now subcommands of `workflow` (issue #305).
//!
//! ## Nested-command note
//! The clap adapter now dispatches nested paths via `extract_nested_command_path`
//! and the tree-based `build_clap_root` (cli-framework rev ≥ 0b2b1b2).  Group
//! commands (workflow, webhook) are registered at root level and dispatch
//! internally via their first positional arg.  The `data` group is registered
//! as a true clap subcommand tree with one leaf per HTTP verb.
//! `workflow` handles validate/lint/preview/graph/resume/runs/checkpoint/artifact.
//!
//! ## CLI command naming patterns
//!
//! Newton supports two positional-dispatch patterns:
//!
//! **Group-first** (`workflow runs list`, `workflow checkpoint clean`): The first
//! positional arg selects a subcommand group, the second selects the action.
//! Use when the resource/group is the primary noun (workflow, webhook).
//!
//! **Verb-first** (`data get product <id>`, `data post product`): The `data`
//! group now uses native clap subcommands: `newton data get`, `newton data post`,
//! etc.  Each verb is a separate leaf command with per-verb help, examples, and
//! restricted flag sets.
//!
//! Both patterns are supported in the framework and may be used for future commands.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::app::{App, AppBuilder};
use cli_framework::command::{Command, CommandArgs};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::{CommandPath, CommandSpec, GroupMetadata};
use uuid::Uuid;

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, BatchArgs, CheckpointArgs, CheckpointCommand, DataArgs,
    DataVerb, DotArgs, ExplainArgs, GraphFormat, InitArgs, LintArgs, OutputFormat, ResumeArgs,
    RunArgs, RunsArgs, RunsCommand, ServeArgs, ValidateArgs, WebhookArgs, WebhookCommand,
    WebhookServeArgs, WebhookStatusArgs,
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
    /// `serve --with-embedded-ailoop`: invalid `--ailoop-base-path` shape (issue #351).
    pub const NEWTON_SERVE_AIL_001: &str = "NEWTON-SERVE-AIL-001";
    /// `serve --with-embedded-ailoop`: `--ailoop-base-path` collides with Newton REST prefix (issue #351).
    pub const NEWTON_SERVE_AIL_002: &str = "NEWTON-SERVE-AIL-002";
    /// `serve --with-embedded-ailoop`: `--ailoop-base-path` collides with `--mcp-path` (issue #351).
    pub const NEWTON_SERVE_AIL_003: &str = "NEWTON-SERVE-AIL-003";
    /// `serve --with-embedded-ailoop`: ailoop_server::router() returned an error (issue #351).
    pub const NEWTON_SERVE_AIL_004: &str = "NEWTON-SERVE-AIL-004";
}

// ── help-text constants ───────────────────────────────────────────────────────

pub const WORKFLOW_RUN_LONG_ABOUT: &str = "\
Run executes a workflow graph defined in YAML, with optional trigger payload.

EXAMPLES:
  Basic workflow execution:
    newton workflow run workflow.yaml

  With workspace and trigger data:
    newton workflow run workflow.yaml --workspace ./output --trigger key=value

  Multiple trigger arguments:
    newton workflow run workflow.yaml --trigger env=prod --trigger version=1.2.3

  With input file and verbose output:
    newton workflow run workflow.yaml input.txt --workspace ./workspace --verbose

  With base trigger payload from a JSON file:
    newton workflow run workflow.yaml --parameters-json payload.json --trigger override=1";

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

const WORKFLOW_LONG_ABOUT: &str = "\
Workflow groups all commands for operating on workflow YAML files and managing \
the execution lifecycle: run, validate, lint, preview, graph, resume, runs, \
checkpoint, and artifact.

Subcommands (execution):
  run <FILE>         Execute a workflow graph

Subcommands (file-oriented):
  validate <FILE>    Validate a workflow graph definition
  lint <FILE>        Check workflow for best practices and issues
  preview <FILE>     Preview what running the workflow would do
  graph <FILE>       Render the workflow graph (default --format dot)

Subcommands (execution-lifecycle):
  resume             Continue a workflow from its last checkpoint (--run-id)
  runs list          List workflow execution history
  runs show          Show task-by-task detail for a specific run (--run-id)
  checkpoint list    Display available executions and checkpoint details
  checkpoint clean   Remove old checkpoint files (--older-than)
  artifact clean     Remove old execution artifact files (--older-than)

EXAMPLES:
  newton workflow run workflow.yaml
  newton workflow run workflow.yaml --workspace ./output --trigger key=value
  newton workflow validate workflow.yaml
  newton workflow lint workflow.yaml --format json
  newton workflow preview workflow.yaml --trigger env=prod --format prose
  newton workflow graph workflow.yaml --output graph.dot
  newton workflow resume --run-id 12345678-1234-1234-1234-123456789abc
  newton workflow runs list --workspace ./workspace
  newton workflow runs show --run-id <RUN_ID> --task my-task --verbose
  newton workflow checkpoint list --workspace ./workspace --json
  newton workflow checkpoint clean --workspace ./workspace --older-than 7d
  newton workflow artifact clean --workspace ./workspace --older-than 30d";

const WEBHOOK_LONG_ABOUT: &str = "\
Webhook provides HTTP endpoints that can trigger workflow executions in \
response to external events.

Subcommands:
  serve   Start an HTTP server to receive webhook events
  status  Display webhook endpoint configuration and status

EXAMPLES:
  newton webhook serve --workflow workflow.yaml --workspace ./workspace
  newton webhook status --workflow workflow.yaml --workspace ./workspace";

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
        summary: "Execute a workflow graph (deprecated — use `newton workflow run`)",
        syntax: Some("<WORKFLOW> [INPUT_FILE] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Execute a workflow graph (deprecated — use `newton workflow run`)",
            long_about: Some(WORKFLOW_RUN_LONG_ABOUT),
            hidden: true,
            examples: vec![
                "newton workflow run workflow.yaml",
                "newton workflow run workflow.yaml --workspace ./output --trigger key=value",
                "newton workflow run workflow.yaml --trigger env=prod --trigger version=1.2.3",
                "newton workflow run workflow.yaml input.txt --workspace ./workspace --verbose",
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
                    name: "parameters-json",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("parameters-json"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Load JSON object as base parameters before --trigger overrides. Accepts a bare path or @path syntax.",
                },
                ArgSpec {
                    name: "emit-completion-json",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("emit-completion-json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Write structured completion envelope to stdout as JSON",
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
                eprintln!(
                    "[newton] DEPRECATED: `newton run` is deprecated; \
                     use `newton workflow run` instead"
                );
                let run_args = RunArgs::try_from(args)?;
                commands::workflow_run(run_args).await.map_err(anyhow::Error::from)
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
                ArgSpec {
                    name: "with-embedded-ailoop",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("with-embedded-ailoop"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Embed the ailoop HTTP/WebSocket server on the same listener as the Newton API",
                },
                ArgSpec {
                    name: "ailoop-base-path",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ailoop-base-path"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path prefix where the embedded ailoop router is mounted (default: /ailoop). Must not be `/api`.",
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

// ── Per-verb help content ──────────────────────────────────────────────────────

const DATA_GET_LONG_ABOUT: &str =
    "Retrieve catalog entities — either a full collection or a single item by id.\n\n\
     EXAMPLES:\n  \
     newton data get products\n  \
     newton data get product <id> --json\n  \
     newton data get grades\n  \
     newton data get grade <id>";

const DATA_POST_LONG_ABOUT: &str =
    "Create a new catalog entity.  Do not pass an id; the server assigns one.\n\n\
     EXAMPLES:\n  \
     newton data post product -f body.json\n  \
     newton data post component -f body.json --dry-run\n  \
     newton data post grade -f grade.json";

const DATA_PUT_LONG_ABOUT: &str =
    "Replace an existing catalog entity (full update).  The entity id is required.\n\n\
     EXAMPLES:\n  \
     newton data put product <id> -f body.json";

const DATA_PATCH_LONG_ABOUT: &str =
    "Partially update an existing catalog entity.  The entity id is required.\n\n\
     EXAMPLES:\n  \
     newton data patch product <id> --body '{\"name\":\"X\"}'\n  \
     newton data patch grade <id> --body '{\"score\":88}'";

const DATA_DELETE_LONG_ABOUT: &str = "Delete a catalog entity by id.\n\n\
     EXAMPLES:\n  \
     newton data delete product <id>\n  \
     newton data delete grade <id>";

/// Build a clap-registered leaf `Command` for a single HTTP verb.
fn data_verb_command(verb: DataVerb) -> Command {
    let (id, summary, long_about, examples, syntax, has_body_args) = match verb {
        DataVerb::Get => (
            "get",
            "Retrieve catalog entities (list or single-item)",
            DATA_GET_LONG_ABOUT,
            vec!["newton data get products", "newton data get product <id> --json"],
            "get <resource> [ID] [--json] [--output-format FORMAT] [--workspace PATH]",
            false,
        ),
        DataVerb::Post => (
            "post",
            "Create a new catalog entity",
            DATA_POST_LONG_ABOUT,
            vec![
                "newton data post product -f body.json",
                "newton data post component -f body.json --dry-run",
            ],
            "post <resource> [--file FILE | --body JSON] [--dry-run] [--json] [--workspace PATH]",
            true,
        ),
        DataVerb::Put => (
            "put",
            "Replace a catalog entity (full update)",
            DATA_PUT_LONG_ABOUT,
            vec!["newton data put product <id> -f body.json"],
            "put <resource> <ID> [--file FILE | --body JSON] [--dry-run] [--json] [--workspace PATH]",
            true,
        ),
        DataVerb::Patch => (
            "patch",
            "Partially update a catalog entity",
            DATA_PATCH_LONG_ABOUT,
            vec!["newton data patch product <id> --body '{\"name\":\"X\"}'"],
            "patch <resource> <ID> [--file FILE | --body JSON] [--dry-run] [--json] [--workspace PATH]",
            true,
        ),
        DataVerb::Delete => (
            "delete",
            "Delete a catalog entity",
            DATA_DELETE_LONG_ABOUT,
            vec!["newton data delete product <id>"],
            "delete <resource> <ID> [--json] [--workspace PATH]",
            false,
        ),
    };

    let mut args = vec![
        ArgSpec {
            name: "resource",
            kind: ArgKind::Positional,
            short: None,
            long: None,
            value_type: ArgValueType::String,
            cardinality: Cardinality::Required,
            default: None,
            conflicts_with: vec![],
            requires: vec![],
            help: "Resource token (product, products, component, components, \
                   repo, repos, module, modules, module-dependency, module-dependencies, \
                   grade, grades)",
        },
        ArgSpec {
            name: "id",
            kind: ArgKind::Positional,
            short: None,
            long: None,
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec![],
            requires: vec![],
            help: "Entity ID (required for single-item GET and all mutating verbs except POST)",
        },
        ArgSpec {
            name: "json",
            kind: ArgKind::Flag,
            short: Some('j'),
            long: Some("json"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec![],
            requires: vec![],
            help: "Emit machine-readable JSON to stdout",
        },
        ArgSpec {
            name: "output-format",
            kind: ArgKind::Option,
            short: None,
            long: Some("output-format"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec![],
            requires: vec![],
            help: "Output format: text (default) or json (alias for --json)",
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
            help: "Workspace root containing .newton/state/backend.sqlite",
        },
    ];

    if has_body_args {
        args.push(ArgSpec {
            name: "file",
            kind: ArgKind::Option,
            short: Some('f'),
            long: Some("file"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec!["body"],
            requires: vec![],
            help: "Path to JSON body file; use - for stdin",
        });
        args.push(ArgSpec {
            name: "body",
            kind: ArgKind::Option,
            short: None,
            long: Some("body"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec!["file"],
            requires: vec![],
            help: "Inline JSON body string (mutually exclusive with --file)",
        });
        args.push(ArgSpec {
            name: "dry-run",
            kind: ArgKind::Flag,
            short: None,
            long: Some("dry-run"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            default: None,
            conflicts_with: vec![],
            requires: vec![],
            help: "Parse and validate body without writing to DB",
        });
    }

    Command {
        id,
        summary,
        syntax: Some(syntax),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary,
            long_about: Some(long_about),
            examples,
            args,
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(move |_ctx, args| {
            Box::pin(async move {
                let dto = DataArgs::from_verb_and_args(verb, args)?;
                commands::data(dto).await
            })
        }),
        expose_mcp: true,
    }
}

fn workflow_command() -> Command {
    Command {
        id: "workflow",
        summary: "Operate on workflow YAML files or manage execution lifecycle (validate/lint/preview/graph/run/resume/runs/checkpoint/artifact)",
        syntax: Some("<validate|lint|preview|graph|run|resume|runs|checkpoint|artifact> [SUBCOMMAND] [FILE] [OPTIONS]"),
        category: Some(categories::WORKFLOW),
        spec: Some(Arc::new(CommandSpec {
            summary: "Operate on workflow YAML files or manage execution lifecycle (validate/lint/preview/graph/run/resume/runs/checkpoint/artifact)",
            long_about: Some(WORKFLOW_LONG_ABOUT),
            examples: vec![
                "newton workflow run workflow.yaml",
                "newton workflow run workflow.yaml --workspace ./output --trigger key=value",
                "newton workflow validate workflow.yaml",
                "newton workflow lint workflow.yaml --format json",
                "newton workflow preview workflow.yaml --trigger env=prod --format prose",
                "newton workflow graph workflow.yaml --output graph.dot",
                "newton workflow resume --run-id 12345678-1234-1234-1234-123456789abc",
                "newton workflow runs list --workspace ./workspace",
                "newton workflow runs show --run-id <RUN_ID> --task my-task --verbose",
                "newton workflow checkpoint list --workspace ./workspace --json",
                "newton workflow checkpoint clean --workspace ./workspace --older-than 7d",
                "newton workflow artifact clean --workspace ./workspace --older-than 30d",
            ],
            args: vec![
                // Positional 1: required first-level subcommand
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec![
                        "validate", "lint", "preview", "graph", "run",
                        "resume", "runs", "checkpoint", "artifact",
                    ]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: validate | lint | preview | graph | run | resume | runs | checkpoint | artifact",
                },
                // Positional 2: second-level subcommand (runs/checkpoint/artifact) or file path (validate/lint/preview/graph)
                ArgSpec {
                    name: "subcommand2",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Second-level subcommand (runs: list|show; checkpoint: list|clean; artifact: clean) or workflow file path (validate/lint/preview/graph)",
                },
                // Positional 3: input file for `workflow run`; declared per spec §4.3.
                // For validate/lint/preview/graph the file lands in subcommand2 (slot 2),
                // so this slot is only populated when running `workflow run wf.yaml input.txt`.
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
                    help: "Optional input file path (workflow run only)",
                },
                // Named options — pre-existing
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
                    help: "Workspace root directory",
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
                    name: "parameters-json",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("parameters-json"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "JSON file with base trigger payload (preview/workflow run). Accepts a bare path or @path syntax.",
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
                // New named options from relocated commands
                ArgSpec {
                    name: "run-id",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("run-id"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "UUID of the workflow run to resume (resume) or inspect (runs show)",
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
                    help: "Emit machine-readable JSON (checkpoint list, runs list)",
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
                    help: "Duration threshold for clean (e.g. 7d, 1w, 24h)",
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
                    help: "Limit list to N most recent executions (runs list)",
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
                    help: "Filter output to a single task ID (runs show)",
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
                    help: "Expand single-task output for debugging (runs show) or workflow run",
                },
                // Run-specific options (workflow run subcommand)
                ArgSpec {
                    name: "emit-completion-json",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("emit-completion-json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Write structured completion envelope to stdout as JSON (workflow run)",
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
                    help: "Runtime override for bounded task concurrency (workflow run)",
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
                    help: "Runtime wall-clock limit override in seconds (workflow run)",
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
                    help: "Newton server URL to register this run (workflow run)",
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
                        let workflow = get_opt_path(&args, "subcommand2").ok_or_else(|| {
                            anyhow!(
                                "{}: workflow file is required for workflow validate",
                                error_codes::CLI_MIG_002
                            )
                        })?;
                        commands::validate(ValidateArgs { workflow }).map_err(anyhow::Error::from)
                    }
                    "lint" => {
                        let workflow = get_opt_path(&args, "subcommand2").ok_or_else(|| {
                            anyhow!(
                                "{}: workflow file is required for workflow lint",
                                error_codes::CLI_MIG_002
                            )
                        })?;
                        commands::lint(LintArgs {
                            workflow,
                            format: parse_output_format(&args),
                        })
                        .map_err(anyhow::Error::from)
                    }
                    "preview" => {
                        let workflow = get_opt_path(&args, "subcommand2").ok_or_else(|| {
                            anyhow!(
                                "{}: workflow file is required for workflow preview",
                                error_codes::CLI_MIG_002
                            )
                        })?;
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
                            parameters_json: get_opt_path(&args, "parameters-json"),
                        })
                        .map_err(anyhow::Error::from)
                    }
                    "graph" => {
                        let workflow = get_opt_path(&args, "subcommand2").ok_or_else(|| {
                            anyhow!(
                                "{}: workflow file is required for workflow graph",
                                error_codes::CLI_MIG_002
                            )
                        })?;
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
                    "resume" => {
                        let dto = ResumeArgs::try_from(args)?;
                        commands::resume(dto).await.map_err(anyhow::Error::from)
                    }
                    "checkpoint" => {
                        let subcmd2 = args
                            .named
                            .get("subcommand2")
                            .map(String::as_str)
                            .unwrap_or("");
                        match subcmd2 {
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
                                subcmd2
                            )),
                        }
                    }
                    "artifact" => {
                        let subcmd2 = args
                            .named
                            .get("subcommand2")
                            .map(String::as_str)
                            .unwrap_or("");
                        match subcmd2 {
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
                                subcmd2
                            )),
                        }
                    }
                    "runs" => {
                        let subcmd2 = args
                            .named
                            .get("subcommand2")
                            .map(String::as_str)
                            .unwrap_or("")
                            .to_string();
                        match subcmd2.as_str() {
                            "list" => {
                                let last = args
                                    .named
                                    .get("last")
                                    .map(|s| {
                                        let n: usize = s.parse().map_err(|_| {
                                            anyhow!(
                                                "LOG-003: runs list --last must be a positive integer"
                                            )
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
                                let run_id_str =
                                    args.named.get("run-id").cloned().ok_or_else(|| {
                                        anyhow!(
                                            "{}: <RUN_ID> is required for `runs show`",
                                            error_codes::CLI_MIG_002
                                        )
                                    })?;
                                let run_id = Uuid::parse_str(&run_id_str).map_err(|e| {
                                    anyhow!(
                                        "{}: invalid run-id UUID: {}",
                                        error_codes::CLI_MIG_002,
                                        e
                                    )
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
                                subcmd2
                            )),
                        }
                    }
                    "run" => {
                        let workflow =
                            get_opt_path(&args, "subcommand2").ok_or_else(|| {
                                anyhow!(
                                    "{}: workflow file is required for workflow run",
                                    error_codes::CLI_MIG_002
                                )
                            })?;
                        let input_file = get_opt_path(&args, "input-file");
                        let trigger = parse_kvp_list(
                            args.named.get("trigger").map(String::as_str).unwrap_or(""),
                        )?;
                        let context = parse_kvp_list(
                            args.named.get("context").map(String::as_str).unwrap_or(""),
                        )?;
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
                        let run_args = RunArgs {
                            workflow,
                            input_file,
                            workspace: get_opt_path(&args, "workspace"),
                            trigger,
                            context,
                            parameters_json: get_opt_path(&args, "parameters-json"),
                            emit_completion_json: get_bool(&args, "emit-completion-json"),
                            parallel_limit,
                            timeout_seconds,
                            verbose: get_bool(&args, "verbose"),
                            server: get_opt_str(&args, "server"),
                        };
                        commands::workflow_run(run_args).await.map_err(anyhow::Error::from)
                    }
                    _ => Err(anyhow!(
                        "{}: unknown workflow subcommand '{}'",
                        error_codes::CLI_MIG_005,
                        subcmd
                    )),
                }
            })
        }),
        expose_mcp: true,
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
        expose_mcp: true,
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
        expose_mcp: true,
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
        workflow_command(),
        webhook_command(),
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
    use cli_framework::mcp::{
        CliFrameworkHandler, McpToolExportPolicy, McpToolRegistry, McpTransportKind,
    };
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    // Build the same tree registry that `build_app` populates so MCP tool
    // names line up exactly with Newton's `--mcp-serve` mode.
    let registry = build_mcp_command_registry()
        .map_err(|e| anyhow!("{}: {e}", error_codes::NEWTON_SERVE_MCP_004))?;

    let tool_registry = std::sync::Arc::new(McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    ));
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
                Ok(CliFrameworkHandler::new(
                    std::sync::Arc::clone(&tool_registry),
                    McpTransportKind::Http,
                ))
            }
        },
        session_manager,
        config,
    );

    Ok(axum::Router::new().nest_service(mcp_path, service))
}

/// Build the Newton CLI application backed by `cli-framework`.
pub fn build_app(ctx: NewtonContext) -> anyhow::Result<App<NewtonContext>> {
    use cli_framework::mcp::McpToolExportPolicy;
    let builder = AppBuilder::new().with_version("newton", env!("CARGO_PKG_VERSION"));
    #[allow(unused_mut)]
    let mut builder = populate_command_registry(builder)?;
    #[cfg(feature = "ask")]
    {
        builder = builder.register_command(ask_command())?;
    }
    builder
        .with_mcp_export_policy(McpToolExportPolicy::ExposeMcpOnly)
        .build(ctx)
        .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_001, e))
}

/// Stable list of tree-path strings registered by [`build_app`].
/// `resume`, `runs`, `checkpoint`, and `artifact` are subcommands of `workflow` (issue #305).
/// `data` is a group; its five verb leaves appear as path strings (issue #336).
pub const REGISTERED_COMMAND_IDS: &[&str] = &[
    "run",
    "init",
    "batch",
    "serve",
    "workflow",
    "webhook",
    "health",
    "doctor",
    "config",
    "completion",
    // data group leaves — "data" itself is a group node, not a leaf
    "data/get",
    "data/post",
    "data/put",
    "data/patch",
    "data/delete",
];

/// Commands exposed as MCP tools under the ExposeMcpOnly policy (issue #309, #336).
/// Entries use dot-separated path notation matching MCP tool naming (e.g. `newton.data.get`).
/// `data` is replaced by the five verb leaf tools (issue #336 — BREAKING CHANGE).
pub const MCP_EXPOSED_COMMAND_IDS: &[&str] = &[
    "config",
    "data.get",
    "data.post",
    "data.put",
    "data.patch",
    "data.delete",
    "health",
    "workflow",
];

/// Returns root-level `Command`s registered by [`build_app`], in registration order.
///
/// NOTE: Superseded by [`build_mcp_command_registry`] for MCP use and
/// [`enumerate_tree_commands`] for complete metadata inspection.
/// The `data` verb leaves are NOT included here; they are registered as nested
/// tree commands under the "data" group.
pub fn enumerate_commands() -> Vec<Command> {
    #[allow(unused_mut)]
    let mut cmds = vec![
        run_command(),
        init_command(),
        batch_command(),
        serve_command(),
        workflow_command(),
        webhook_command(),
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

/// Returns all leaf commands with their full path strings (slash-separated), suitable
/// for metadata inspection and test assertions on the tree layout.
pub fn enumerate_tree_commands() -> Vec<(String, Command)> {
    let registry = build_mcp_command_registry()
        .expect("failed to build command registry for tree enumeration");
    let mut items: Vec<(String, Command)> = registry
        .all_tree_commands()
        .map(|(path, cmd)| (path.to_string(), cmd.clone()))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    #[cfg(feature = "ask")]
    items.push(("ask".to_string(), ask_command()));
    items
}

/// Build the full tree `CommandRegistry` used for MCP tool registration and
/// the `newton serve --with-mcp` router.  Both `build_app` and
/// `build_mcp_router_for_serve` derive their registrations from this function.
pub fn build_mcp_command_registry(
) -> anyhow::Result<cli_framework::command::registry::CommandRegistry> {
    use cli_framework::command::registry::CommandRegistry;

    let mut registry = CommandRegistry::new();

    // Register root-level non-data commands
    for cmd in [
        run_command(),
        init_command(),
        batch_command(),
        serve_command(),
        workflow_command(),
        webhook_command(),
        health_command(),
        doctor_command(),
        config_command(),
        completion_command(),
    ] {
        registry.register(cmd);
    }

    // Register `data` group and its five verb leaf commands
    let data_path = CommandPath::new(&["data"]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
    registry
        .register_group(
            &data_path,
            GroupMetadata {
                summary: "Catalog CRUD via HTTP-style verbs (get/post/put/patch/delete)",
                hidden: false,
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

    for verb in [
        DataVerb::Get,
        DataVerb::Post,
        DataVerb::Put,
        DataVerb::Patch,
        DataVerb::Delete,
    ] {
        let path =
            CommandPath::new(&["data", verb.as_str()]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
        registry
            .register_at(&path, data_verb_command(verb))
            .map_err(|e| anyhow!("{e}"))?;
    }

    Ok(registry)
}

/// Shared registration logic called by both [`build_app`] and (indirectly) by
/// [`build_mcp_router_for_serve`] via [`build_mcp_command_registry`].
fn populate_command_registry(builder: AppBuilder) -> anyhow::Result<AppBuilder> {
    let builder = builder
        .register_command(run_command())?
        .register_command(init_command())?
        .register_command(batch_command())?
        .register_command(serve_command())?
        .register_command(workflow_command())?
        .register_command(webhook_command())?
        .register_command(health_command())?
        .register_command(doctor_command())?
        .register_command(config_command())?
        .register_command(completion_command())?;

    let data_path = CommandPath::new(&["data"]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
    let builder = builder.register_group(
        &data_path,
        GroupMetadata {
            summary: "Catalog CRUD via HTTP-style verbs (get/post/put/patch/delete)",
            hidden: false,
        },
    )?;

    [
        DataVerb::Get,
        DataVerb::Post,
        DataVerb::Put,
        DataVerb::Patch,
        DataVerb::Delete,
    ]
    .into_iter()
    .try_fold(builder, |b, verb| {
        let path =
            CommandPath::new(&["data", verb.as_str()]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
        b.register_command_at(&path, data_verb_command(verb))
    })
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
        let parameters_json = get_opt_path(&args, "parameters-json");
        let emit_completion_json = get_bool(&args, "emit-completion-json");
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
            parameters_json,
            emit_completion_json,
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
        let with_embedded_ailoop = get_bool(&args, "with-embedded-ailoop");
        let ailoop_base_path = args
            .named
            .get("ailoop-base-path")
            .cloned()
            .unwrap_or_else(|| "/ailoop".to_string());
        Ok(ServeArgs {
            host,
            port,
            static_ui: get_opt_path(&args, "static-ui"),
            with_mcp,
            mcp_path,
            with_embedded_ailoop,
            ailoop_base_path,
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

impl DataArgs {
    /// Construct `DataArgs` from a `CommandArgs` bag where the verb is already known
    /// (supplied by the leaf command's execute closure, not parsed from positionals).
    pub fn from_verb_and_args(verb: DataVerb, args: CommandArgs) -> Result<Self, anyhow::Error> {
        let resource = args
            .named
            .get("resource")
            .cloned()
            .ok_or_else(|| anyhow!("DATA-003: resource token is required"))?;
        let id = args.named.get("id").cloned();
        let file = args.named.get("file").map(PathBuf::from);
        let body = args.named.get("body").cloned();
        let json = get_bool(&args, "json")
            || args
                .named
                .get("output-format")
                .map(|s| s == "json")
                .unwrap_or(false);
        let dry_run = get_bool(&args, "dry-run");
        let workspace = get_opt_path(&args, "workspace");
        Ok(DataArgs {
            verb,
            resource,
            id,
            file,
            body,
            json,
            dry_run,
            workspace,
        })
    }
}

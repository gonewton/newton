//! cli-framework registration for Newton CLI.
//! Decomposed into submodules by concern; this file provides shared helpers,
//! public entry points, and TryFrom adapters.

pub(crate) mod commands;
pub mod error_codes;
pub mod mcp;

#[path = "../help_text.rs"]
pub(crate) mod help_text;

pub use help_text::WORKFLOW_RUN_LONG_ABOUT;
pub use mcp::{build_mcp_command_registry, build_mcp_router_for_serve};

use std::path::PathBuf;

use anyhow::anyhow;
use cli_framework::app::{App, AppBuilder};
use cli_framework::command::{Command, CommandArgs};
use cli_framework::spec::command_tree::{CommandPath, GroupMetadata};
use uuid::Uuid;

use crate::cli::args::{
    BatchArgs, DataArgs, DataVerb, InitArgs, OutputFormat, ResumeArgs, RunArgs, ServeArgs,
};
use crate::cli::context::NewtonContext;

// ── shared helpers used by command submodules ────────────────────────────────

pub(crate) fn parse_kvp_list(s: &str) -> anyhow::Result<Vec<crate::cli::args::KeyValuePair>> {
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

pub(crate) fn get_bool(args: &CommandArgs, key: &str) -> bool {
    args.named.get(key).map(|s| s == "true").unwrap_or(false)
}

pub(crate) fn get_opt_path(args: &CommandArgs, key: &str) -> Option<PathBuf> {
    args.named.get(key).map(PathBuf::from)
}

pub(crate) fn get_opt_str(args: &CommandArgs, key: &str) -> Option<String> {
    args.named.get(key).cloned()
}

pub(crate) fn parse_output_format(args: &CommandArgs) -> OutputFormat {
    match args.named.get("format").map(String::as_str) {
        Some("json") => OutputFormat::Json,
        Some("prose") => OutputFormat::Prose,
        _ => OutputFormat::Text,
    }
}

pub(crate) fn require_workflow_path(args: &CommandArgs, label: &str) -> anyhow::Result<PathBuf> {
    get_opt_path(args, "workflow").ok_or_else(|| {
        anyhow!(
            "{}: workflow file is required for {}",
            error_codes::CLI_MIG_002,
            label
        )
    })
}

// ── command registry helpers ─────────────────────────────────────────────────

fn all_root_commands() -> Vec<Command> {
    vec![
        commands::run::run_command(),
        commands::init::init_command(),
        commands::batch::batch_command(),
        commands::serve::serve_command(),
        commands::ops::health_command(),
        commands::ops::doctor_command(),
        commands::ops::config_command(),
        commands::workflow::webhook_command(),
        commands::workflow::workflow_command(),
    ]
}

fn populate_command_registry(builder: AppBuilder) -> anyhow::Result<AppBuilder> {
    let builder = all_root_commands()
        .into_iter()
        .try_fold(builder, |b, cmd| b.register_command(cmd))?;

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
        b.register_command_at(&path, commands::data::data_verb_command(verb))
    })
}

// ── public entry points ──────────────────────────────────────────────────────

/// Build the Newton CLI application backed by `cli-framework`.
pub fn build_app(ctx: NewtonContext) -> anyhow::Result<App<NewtonContext>> {
    use cli_framework::mcp::McpToolExportPolicy;
    let builder = AppBuilder::new().with_version("newton", env!("CARGO_PKG_VERSION"));
    let builder = populate_command_registry(builder)?;
    builder
        .with_mcp_export_policy(McpToolExportPolicy::ExposeMcpOnly)
        .build(ctx)
        .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_001, e))
}

/// Stable list of tree-path strings registered by [`build_app`].
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
    "data/get",
    "data/post",
    "data/put",
    "data/patch",
    "data/delete",
];

/// Commands exposed as MCP tools under the ExposeMcpOnly policy.
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

pub fn enumerate_commands() -> Vec<Command> {
    all_root_commands()
}

/// Returns all leaf commands with their full path strings (slash-separated).
pub fn enumerate_tree_commands() -> Vec<(String, Command)> {
    let registry = mcp::build_mcp_command_registry()
        .expect("failed to build command registry for tree enumeration");
    let mut items: Vec<(String, Command)> = registry
        .all_tree_commands()
        .map(|(path, cmd)| (path.to_string(), cmd.clone()))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// Returns all leaf commands present in the fully built app registry.
pub fn enumerate_effective_app_tree_commands() -> Vec<(String, Command)> {
    let app =
        build_app(NewtonContext::new()).expect("failed to build app for registry enumeration");
    let mut items: Vec<(String, Command)> = app
        .command_registry()
        .all_tree_commands()
        .map(|(path, cmd)| (path.to_string(), cmd.clone()))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

// ── TryFrom<CommandArgs> adapters ────────────────────────────────────────────

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
        let state_dir = get_opt_path(&args, "state-dir");
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
            state_dir,
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
            with_embedded_ailoop,
            ailoop_base_path,
            state_dir: get_opt_path(&args, "state-dir"),
            import_existing: get_bool(&args, "import-existing"),
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
            state_dir: get_opt_path(&args, "state-dir"),
        })
    }
}

impl DataArgs {
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
        let run_id = args.named.get("run-id").cloned();
        let kpi_id = args.named.get("kpi-id").cloned();
        let scope = args.named.get("scope").cloned();
        let scope_id = args.named.get("scope-id").cloned();
        let source = args.named.get("source").cloned();
        let limit = args
            .named
            .get("limit")
            .map(|s| {
                s.parse::<u32>()
                    .map_err(|_| anyhow!("DATA-007: --limit must be a positive integer"))
                    .and_then(|n| {
                        if n == 0 {
                            Err(anyhow!("DATA-007: --limit must be a positive integer"))
                        } else {
                            Ok(n)
                        }
                    })
            })
            .transpose()?;
        Ok(DataArgs {
            verb,
            resource,
            id,
            file,
            body,
            json,
            dry_run,
            workspace,
            run_id,
            kpi_id,
            scope,
            scope_id,
            source,
            limit,
        })
    }
}

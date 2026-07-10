//! cli-framework registration for Newton CLI.
//! Decomposed into submodules by concern; this file provides shared helpers,
//! public entry points, and FromArgValueMap adapters.

pub(crate) mod commands;
pub mod error_codes;
pub mod mcp;

#[path = "../help_text.rs"]
pub(crate) mod help_text;

pub use help_text::WORKFLOW_RUN_LONG_ABOUT;
pub use mcp::{build_mcp_command_registry, build_mcp_router_for_serve};

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::anyhow;
use cli_framework::app::{App, AppBuilder};
use cli_framework::command::Command;
use cli_framework::command::FromArgValueMap;
use cli_framework::spec::command_tree::{CommandPath, GroupMetadata};
use cli_framework::spec::value::ArgValue;
use uuid::Uuid;

use crate::cli::args::{
    DataArgs, DataVerb, InitArgs, OptimizeArgs, OutputFormat, ResumeArgs, RunArgs, ServeArgs,
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

/// Extract a Repeated KVP option from an ArgValue map.
///
/// Prefers `List` (populated by cli-framework spec-based parsing)
/// over the legacy comma-joined string, so that multiple `--trigger`
/// flags each arrive as their own `KeyValuePair` rather than being silently
/// dropped.
pub(crate) fn parse_kvp_from_map(
    map: &HashMap<String, ArgValue>,
    key: &str,
) -> anyhow::Result<Vec<crate::cli::args::KeyValuePair>> {
    use std::str::FromStr;
    match map.get(key) {
        Some(ArgValue::List(items)) => {
            return items
                .iter()
                .map(|v| match v {
                    ArgValue::Str(s) => crate::cli::args::KeyValuePair::from_str(s)
                        .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_002, e)),
                    _other => Err(anyhow!(
                        "{}: expected string value for --{}, got unexpected type",
                        error_codes::CLI_MIG_002,
                        key,
                    )),
                })
                .collect();
        }
        Some(ArgValue::Str(s)) => {
            return parse_kvp_list(s);
        }
        _ => {}
    }
    Ok(vec![])
}

pub(crate) fn get_bool(map: &HashMap<String, ArgValue>, key: &str) -> bool {
    matches!(map.get(key), Some(ArgValue::Bool(true)))
}

pub(crate) fn get_opt_path(map: &HashMap<String, ArgValue>, key: &str) -> Option<PathBuf> {
    if let Some(ArgValue::Str(s)) = map.get(key) {
        Some(PathBuf::from(s))
    } else {
        None
    }
}

pub(crate) fn get_opt_str(map: &HashMap<String, ArgValue>, key: &str) -> Option<String> {
    match map.get(key) {
        Some(ArgValue::Str(s)) => Some(s.clone()),
        Some(ArgValue::Enum(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn parse_output_format(map: &HashMap<String, ArgValue>) -> OutputFormat {
    match get_opt_str(map, "format").as_deref() {
        Some("json") => OutputFormat::Json,
        Some("prose") => OutputFormat::Prose,
        _ => OutputFormat::Text,
    }
}

pub(crate) fn require_workflow_path(
    map: &HashMap<String, ArgValue>,
    label: &str,
) -> anyhow::Result<PathBuf> {
    get_opt_path(map, "workflow").ok_or_else(|| {
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
        commands::init::init_command(),
        commands::optimize::optimize_command(),
        commands::serve::serve_command(),
        commands::ops::doctor_command(),
        commands::ops::config_command(),
        commands::workflow::workflow_command(),
        commands::schema::schema_command(),
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
    use cli_framework::command::chat::ChatToolPolicy;
    use cli_framework::mcp::McpToolExportPolicy;
    let builder = AppBuilder::new().with_version("newton", env!("CARGO_PKG_VERSION"));
    let builder = populate_command_registry(builder)?;
    builder
        .with_mcp_export_policy(McpToolExportPolicy::ExposeMcpOnly)
        .with_chat_tool_policy(ChatToolPolicy::UseCommandFlag)
        .build(ctx)
        .map_err(|e| anyhow!("{}: {}", error_codes::CLI_MIG_001, e))
}

/// Stable list of tree-path strings registered by [`build_app`].
pub const REGISTERED_COMMAND_IDS: &[&str] = &[
    "init",
    "optimize",
    "serve",
    "workflow",
    "doctor",
    "config",
    "schema",
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

// ── FromArgValueMap adapters ─────────────────────────────────────────────────

impl RunArgs {
    /// Fallible counterpart to the (infallible-by-contract) `FromArgValueMap`
    /// trait. `workflow`, `trigger`, and `context` are not covered by
    /// cli-framework's static cardinality validation — `workflow` is
    /// assembled from a positional promotion at the call site rather than
    /// being a `Cardinality::Required` arg, and `trigger`/`context` are
    /// free-form `KEY=VALUE` strings the spec only knows as `String` — so
    /// malformed user input (missing workflow file, `--trigger foo` with no
    /// `=`) can genuinely reach this constructor. Return a clean `anyhow`
    /// error instead of panicking (spec 074, B19).
    pub(crate) fn try_from_arg_value_map(map: &HashMap<String, ArgValue>) -> anyhow::Result<Self> {
        let workflow = require_workflow_path(map, "run")?;
        let input_file = get_opt_path(map, "input-file");
        let workspace = get_opt_path(map, "workspace");
        let trigger = parse_kvp_from_map(map, "trigger")
            .map_err(|e| anyhow!("{}: invalid --trigger: {e}", error_codes::CLI_MIG_002))?;
        let context = parse_kvp_from_map(map, "context")
            .map_err(|e| anyhow!("{}: invalid --context: {e}", error_codes::CLI_MIG_002))?;
        let parameters_json = get_opt_path(map, "parameters-json");
        let emit_completion_json = get_bool(map, "emit-completion-json");
        let parallel_limit = if let Some(ArgValue::Int(n)) = map.get("parallel-limit") {
            Some(*n as usize)
        } else {
            None
        };
        let timeout_seconds = if let Some(ArgValue::Int(n)) = map.get("timeout") {
            Some(*n as u64)
        } else {
            None
        };
        let verbose = get_bool(map, "verbose");
        let server = get_opt_str(map, "server");
        let state_dir = get_opt_path(map, "state-dir");
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

impl FromArgValueMap for InitArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        InitArgs {
            path: get_opt_path(map, "path"),
            template: get_opt_str(map, "template"),
        }
    }
}

impl FromArgValueMap for OptimizeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        // Unlike RunArgs::workflow / ResumeArgs::run_id, `project-id` genuinely
        // is `Cardinality::Required` in `optimize_command()`'s spec (see
        // `commands/optimize.rs`), so cli-framework's `validate_typed_args`
        // rejects an invocation missing it *before* this constructor ever
        // runs — this really is the "framework bug" case the trait's own
        // doc comment describes, not user-controllable input (audited for
        // spec 074, B19: left as a panic on purpose).
        let project_id = get_opt_str(map, "project-id")
            .unwrap_or_else(|| panic!("fw bug: project-id is required"));
        let poll_interval_seconds = if let Some(ArgValue::Int(n)) = map.get("poll-interval") {
            *n as u64
        } else {
            60
        };
        OptimizeArgs {
            project_id,
            workspace: get_opt_path(map, "workspace"),
            once: get_bool(map, "once"),
            poll_interval_seconds,
        }
    }
}

impl FromArgValueMap for ServeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        let host = get_opt_str(map, "host").unwrap_or_else(|| "127.0.0.1".to_string());
        let port = if let Some(ArgValue::Int(n)) = map.get("port") {
            u16::try_from(*n).unwrap_or(8080)
        } else {
            8080
        };
        let with_mcp = get_bool(map, "with-mcp");
        let with_embedded_ailoop = get_bool(map, "with-embedded-ailoop");
        let ailoop_base_path =
            get_opt_str(map, "ailoop-base-path").unwrap_or_else(|| "/ailoop".to_string());
        ServeArgs {
            host,
            port,
            no_web: get_bool(map, "no-web"),
            with_mcp,
            with_embedded_ailoop,
            ailoop_base_path,
            state_dir: get_opt_path(map, "state-dir"),
            import_existing: get_bool(map, "import-existing"),
        }
    }
}

impl ResumeArgs {
    /// Fallible counterpart to the (infallible-by-contract) `FromArgValueMap`
    /// trait. `run-id` is `Cardinality::Optional` in the shared `workflow`
    /// command spec (it is reused by both `resume` and `runs show`), so it
    /// can genuinely be absent, and its UUID format is never validated by
    /// the arg spec — mirrors the clean `anyhow!` validation `runs show`
    /// already does (spec 074, B19).
    pub(crate) fn try_from_arg_value_map(map: &HashMap<String, ArgValue>) -> anyhow::Result<Self> {
        let run_id_str = get_opt_str(map, "run-id").ok_or_else(|| {
            anyhow!(
                "{}: --run-id is required for `workflow resume`",
                error_codes::CLI_MIG_002
            )
        })?;
        let run_id = Uuid::parse_str(&run_id_str)
            .map_err(|e| anyhow!("{}: invalid --run-id UUID: {}", error_codes::CLI_MIG_002, e))?;
        Ok(ResumeArgs {
            run_id,
            workspace: get_opt_path(map, "workspace"),
            allow_workflow_change: get_bool(map, "allow-workflow-change"),
            state_dir: get_opt_path(map, "state-dir"),
        })
    }
}

impl DataArgs {
    pub fn from_verb_and_map(
        verb: DataVerb,
        map: &HashMap<String, ArgValue>,
    ) -> Result<Self, anyhow::Error> {
        let resource = get_opt_str(map, "resource")
            .ok_or_else(|| anyhow!("DATA-003: resource token is required"))?;
        let id = get_opt_str(map, "id");
        let file = get_opt_path(map, "file");
        let body = get_opt_str(map, "body");
        let json = get_bool(map, "json")
            || get_opt_str(map, "output-format")
                .as_deref()
                .map(|s| s == "json")
                .unwrap_or(false);
        let dry_run = get_bool(map, "dry-run");
        let workspace = get_opt_path(map, "workspace");
        let state_dir = get_opt_path(map, "state-dir");
        let run_id = get_opt_str(map, "run-id");
        let kpi_id = get_opt_str(map, "kpi-id");
        let scope = get_opt_str(map, "scope");
        let scope_id = get_opt_str(map, "scope-id");
        let source = get_opt_str(map, "source");
        let limit = if let Some(ArgValue::Str(s)) = map.get("limit") {
            Some(
                s.parse::<u32>()
                    .map_err(|_| anyhow!("DATA-007: --limit must be a positive integer"))
                    .and_then(|n| {
                        if n == 0 {
                            Err(anyhow!("DATA-007: --limit must be a positive integer"))
                        } else {
                            Ok(n)
                        }
                    })?,
            )
        } else {
            None
        };
        Ok(DataArgs {
            verb,
            resource,
            id,
            file,
            body,
            json,
            dry_run,
            workspace,
            state_dir,
            run_id,
            kpi_id,
            scope,
            scope_id,
            source,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_framework::spec::value::ArgValue;
    use std::collections::HashMap;

    fn map_with_list(key: &str, values: Vec<&str>) -> HashMap<String, ArgValue> {
        let mut map = HashMap::new();
        map.insert(
            key.to_string(),
            ArgValue::List(
                values
                    .iter()
                    .map(|s| ArgValue::Str(s.to_string()))
                    .collect(),
            ),
        );
        map
    }

    fn map_with_str(key: &str, value: &str) -> HashMap<String, ArgValue> {
        let mut map = HashMap::new();
        map.insert(key.to_string(), ArgValue::Str(value.to_string()));
        map
    }

    fn empty_map() -> HashMap<String, ArgValue> {
        HashMap::new()
    }

    #[test]
    fn repeated_typed_list_returns_multiple_kvps_in_order() {
        let map = map_with_list("trigger", vec!["board_item_id=PVTI_abc", "skip_draft=true"]);
        let result = parse_kvp_from_map(&map, "trigger").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "board_item_id");
        assert_eq!(result[0].value, "PVTI_abc");
        assert_eq!(result[1].key, "skip_draft");
        assert_eq!(result[1].value, "true");
    }

    #[test]
    fn legacy_comma_joined_str_still_works() {
        let map = map_with_str("trigger", "env=prod,version=1.2.3");
        let result = parse_kvp_from_map(&map, "trigger").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "env");
        assert_eq!(result[0].value, "prod");
        assert_eq!(result[1].key, "version");
        assert_eq!(result[1].value, "1.2.3");
    }

    #[test]
    fn absent_key_returns_empty_vec() {
        let result = parse_kvp_from_map(&empty_map(), "trigger").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn invalid_kvp_in_list_returns_error() {
        let map = map_with_list("trigger", vec!["no-equals-sign"]);
        let result = parse_kvp_from_map(&map, "trigger");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains(error_codes::CLI_MIG_002),
            "expected CLI_MIG_002 in: {msg}"
        );
    }

    #[test]
    fn non_string_list_element_returns_error() {
        let mut map = HashMap::new();
        map.insert(
            "trigger".to_string(),
            ArgValue::List(vec![ArgValue::Int(42)]),
        );
        let result = parse_kvp_from_map(&map, "trigger");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains(error_codes::CLI_MIG_002),
            "expected CLI_MIG_002 in: {msg}"
        );
    }
}

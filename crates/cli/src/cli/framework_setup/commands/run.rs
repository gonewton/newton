use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::RunArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::WORKFLOW_RUN_LONG_ABOUT;

pub(crate) fn run_command() -> Command {
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

use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::{DataArgs, DataVerb};
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::{
    DATA_DELETE_LONG_ABOUT, DATA_GET_LONG_ABOUT, DATA_PATCH_LONG_ABOUT, DATA_POST_LONG_ABOUT,
    DATA_PUT_LONG_ABOUT,
};

pub(crate) fn data_verb_command(verb: DataVerb) -> Command {
    let (id, summary, long_about, examples, syntax, has_body_args) = match verb {
        DataVerb::Get => (
            "get",
            "Retrieve catalog entities (list or single-item)",
            DATA_GET_LONG_ABOUT,
            vec!["newton data get products", "newton data get product <id> --json"],
            "get <resource> [ID] [--run-id RUNID] [--kpi-id KPIID] [--scope SCOPE] [--scope-id SCOPEID] [--source SOURCE] [--limit N] [--json] [--output-format FORMAT] [--workspace PATH]",
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
            value_type: ArgValueType::String,
            cardinality: Cardinality::Required,
            help: "Resource token (product, products, component, components, \
                   repo, repos, module, modules, module-dependency, module-dependencies, \
                   kpi, kpis, eval-run, eval-runs, grade, grades)",
            ..Default::default()
        },
        ArgSpec {
            name: "id",
            kind: ArgKind::Positional,
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Entity ID (required for single-item GET and all mutating verbs except POST)",
            ..Default::default()
        },
        ArgSpec {
            name: "json",
            kind: ArgKind::Flag,
            short: Some('j'),
            long: Some("json"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Emit machine-readable JSON to stdout",
            ..Default::default()
        },
        ArgSpec {
            name: "output-format",
            kind: ArgKind::Option,
            long: Some("output-format"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Output format: text (default) or json (alias for --json)",
            ..Default::default()
        },
        ArgSpec {
            name: "workspace",
            kind: ArgKind::Option,
            long: Some("workspace"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Workspace root containing .newton/state/backend.sqlite",
            ..Default::default()
        },
    ];

    if matches!(verb, DataVerb::Get) {
        args.push(ArgSpec {
            name: "run-id",
            kind: ArgKind::Option,
            long: Some("run-id"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Filter grade listings by EvalRun id (only used with: resource=grades)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "kpi-id",
            kind: ArgKind::Option,
            long: Some("kpi-id"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Filter grade listings by KPI id (only used with: resource=grades)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "scope",
            kind: ArgKind::Option,
            long: Some("scope"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Filter EvalRun listings by scope (only used with: resource=eval-runs)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "scope-id",
            kind: ArgKind::Option,
            long: Some("scope-id"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Filter EvalRun listings by scope id (only used with: resource=eval-runs)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "source",
            kind: ArgKind::Option,
            long: Some("source"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Filter EvalRun listings by source (only used with: resource=eval-runs)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "limit",
            kind: ArgKind::Option,
            long: Some("limit"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Limit EvalRun listings (only used with: resource=eval-runs)",
            ..Default::default()
        });
    }

    if has_body_args {
        args.push(ArgSpec {
            name: "file",
            kind: ArgKind::Option,
            short: Some('f'),
            long: Some("file"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            conflicts_with: vec!["body"],
            help: "Path to JSON body file; use - for stdin",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "body",
            kind: ArgKind::Option,
            long: Some("body"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            conflicts_with: vec!["file"],
            help: "Inline JSON body string (mutually exclusive with --file)",
            ..Default::default()
        });
        args.push(ArgSpec {
            name: "dry-run",
            kind: ArgKind::Flag,
            long: Some("dry-run"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Parse and validate body without writing to DB",
            ..Default::default()
        });
    }

    Command {
        id: id.into(),
        spec: Arc::new(CommandSpec {
            summary,
            syntax: Some(syntax),
            category: Some(categories::WORKFLOW),
            long_about: Some(long_about),
            examples,
            args,
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(move |_ctx, args| {
            Box::pin(async move {
                let dto = DataArgs::from_verb_and_map(verb, &args)?;
                commands::data(dto).await
            })
        }),
        expose_mcp: true,
        expose_chat: true,
    }
}

use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use uuid::Uuid;

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, CheckpointArgs, CheckpointCommand, DotArgs, ExplainArgs,
    GraphFormat, ImportArgs, LintArgs, ResumeArgs, RunArgs, RunsArgs, RunsCommand, ValidateArgs,
};
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::error_codes;
use crate::cli::framework_setup::help_text::WORKFLOW_LONG_ABOUT;
use crate::cli::framework_setup::{
    get_bool, get_opt_path, get_opt_str, parse_kvp_from_map, parse_output_format, FromArgValueMap,
};

pub(crate) fn workflow_command() -> Command {
    Command {
        id: "workflow".into(),
        spec: Arc::new(CommandSpec {
            summary: "Operate on workflow YAML files or manage execution lifecycle (validate/lint/preview/graph/run/resume/runs/checkpoint/artifact)",
            syntax: Some("<validate|lint|preview|graph|run|resume|runs|checkpoint|artifact> [SUBCOMMAND] [FILE] [OPTIONS]"),
            category: Some(categories::WORKFLOW),
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
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::Enum(vec![
                        "validate", "lint", "preview", "graph", "run",
                        "resume", "runs", "checkpoint", "artifact", "import",
                    ]),
                    cardinality: Cardinality::Required,
                    help: "Subcommand: validate | lint | preview | graph | run | resume | runs | checkpoint | artifact",
                    ..Default::default()
                },
                ArgSpec {
                    name: "subcommand2",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Second-level subcommand (runs: list|show; checkpoint: list|clean; artifact: clean) or workflow file path (validate/lint/preview/graph)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "input-file",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Optional input file path (workflow run only)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format (lint: text|json; preview: text|json|prose; graph: dot)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Workspace root directory",
                    ..Default::default()
                },
                ArgSpec {
                    name: "context",
                    kind: ArgKind::Option,
                    long: Some("context"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Merge KEY=VALUE into workflow.context at runtime (preview)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "trigger",
                    kind: ArgKind::Option,
                    long: Some("trigger"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Trigger payload override KEY=VALUE (preview)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "parameters-json",
                    kind: ArgKind::Option,
                    long: Some("parameters-json"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "JSON file with base trigger payload (preview/workflow run). Accepts a bare path or @path syntax.",
                    ..Default::default()
                },
                ArgSpec {
                    name: "output",
                    kind: ArgKind::Option,
                    short: Some('o'),
                    long: Some("output"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output destination file (graph)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "run-id",
                    kind: ArgKind::Option,
                    long: Some("run-id"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "UUID of the workflow run to resume (resume) or inspect (runs show)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "allow-workflow-change",
                    kind: ArgKind::Flag,
                    long: Some("allow-workflow-change"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Allow resuming even if the workflow definition changed since checkpoint",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Emit machine-readable JSON (checkpoint list, runs list)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "older-than",
                    kind: ArgKind::Option,
                    long: Some("older-than"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Duration threshold for clean (e.g. 7d, 1w, 24h)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "last",
                    kind: ArgKind::Option,
                    long: Some("last"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Limit list to N most recent executions (runs list)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "task",
                    kind: ArgKind::Option,
                    long: Some("task"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter output to a single task ID (runs show)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "verbose",
                    kind: ArgKind::Flag,
                    short: Some('v'),
                    long: Some("verbose"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Expand single-task output for debugging (runs show) or workflow run",
                    ..Default::default()
                },
                ArgSpec {
                    name: "emit-completion-json",
                    kind: ArgKind::Flag,
                    long: Some("emit-completion-json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Write structured completion envelope to stdout as JSON (workflow run)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "parallel-limit",
                    kind: ArgKind::Option,
                    long: Some("parallel-limit"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Runtime override for bounded task concurrency (workflow run)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "timeout",
                    kind: ArgKind::Option,
                    long: Some("timeout"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Runtime wall-clock limit override in seconds (workflow run)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "server",
                    kind: ArgKind::Option,
                    long: Some("server"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Newton server URL to register this run (workflow run)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "state-dir",
                    kind: ArgKind::Option,
                    long: Some("state-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.",
                    ..Default::default()
                },
                ArgSpec {
                    name: "recursive",
                    kind: ArgKind::Flag,
                    long: Some("recursive"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Recursively walk workspace for all .newton/state/workflows directories (import)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcmd = get_opt_str(&args, "subcommand")
                    .unwrap_or_default();
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
                        let context = parse_kvp_from_map(&args, "context")?;
                        let trigger = parse_kvp_from_map(&args, "trigger")?;
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
                        let format = match get_opt_str(&args, "format").as_deref() {
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
                        let dto = ResumeArgs::from_arg_value_map(&args);
                        commands::resume(dto).await.map_err(anyhow::Error::from)
                    }
                    "checkpoint" => {
                        let subcmd2 = get_opt_str(&args, "subcommand2")
                            .unwrap_or_default();
                        match subcmd2.as_str() {
                            "list" => {
                                let dto = CheckpointArgs {
                                    command: CheckpointCommand::List {
                                        workspace: get_opt_path(&args, "workspace"),
                                        state_dir: get_opt_path(&args, "state-dir"),
                                        json: get_bool(&args, "json"),
                                    },
                                };
                                commands::checkpoints(dto).map_err(anyhow::Error::from)
                            }
                            "clean" => {
                                let older_than =
                                    get_opt_str(&args, "older-than").ok_or_else(|| {
                                        anyhow!(
                                            "{}: --older-than is required for checkpoint clean",
                                            error_codes::CLI_MIG_002
                                        )
                                    })?;
                                let dto = CheckpointArgs {
                                    command: CheckpointCommand::Clean {
                                        workspace: get_opt_path(&args, "workspace"),
                                        state_dir: get_opt_path(&args, "state-dir"),
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
                        let subcmd2 = get_opt_str(&args, "subcommand2")
                            .unwrap_or_default();
                        match subcmd2.as_str() {
                            "clean" => {
                                let older_than =
                                    get_opt_str(&args, "older-than").ok_or_else(|| {
                                        anyhow!(
                                            "{}: --older-than is required for artifact clean",
                                            error_codes::CLI_MIG_002
                                        )
                                    })?;
                                let dto = ArtifactArgs {
                                    command: ArtifactCommand::Clean {
                                        workspace: get_opt_path(&args, "workspace"),
                                        state_dir: get_opt_path(&args, "state-dir"),
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
                        let subcmd2 = get_opt_str(&args, "subcommand2")
                            .unwrap_or_default();
                        match subcmd2.as_str() {
                            "list" => {
                                let last = if let Some(ArgValue::Int(n)) = args.get("last") {
                                    let n = *n as usize;
                                    if n == 0 {
                                        return Err(anyhow!(
                                            "LOG-003: runs list --last must be a positive integer"
                                        ));
                                    }
                                    Some(n)
                                } else {
                                    None
                                };
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
                                    get_opt_str(&args, "run-id").ok_or_else(|| {
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
                        // If workflow key isn't set, promote subcommand2 to workflow
                        let mut map = args;
                        if !map.contains_key("workflow") {
                            if let Some(p) = map.get("subcommand2").cloned() {
                                map.insert("workflow".to_string(), p);
                            }
                        }
                        let run_args = RunArgs::from_arg_value_map(&map);
                        commands::workflow_run(run_args).await
                    }
                    "import" => {
                        let import_args = ImportArgs {
                            state_dir: get_opt_path(&args, "state-dir"),
                            workspace: get_opt_path(&args, "workspace"),
                            recursive: get_bool(&args, "recursive"),
                        };
                        commands::workflow_import(import_args).await.map_err(anyhow::Error::from)
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
        expose_chat: true,
    }
}

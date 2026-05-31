use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use uuid::Uuid;

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, CheckpointArgs, CheckpointCommand, DotArgs, ExplainArgs,
    GraphFormat, ImportArgs, LintArgs, ResumeArgs, RunArgs, RunsArgs, RunsCommand, ValidateArgs,
    WebhookArgs, WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::error_codes;
use crate::cli::framework_setup::help_text::{WEBHOOK_LONG_ABOUT, WORKFLOW_LONG_ABOUT};
use crate::cli::framework_setup::{
    get_bool, get_opt_path, get_opt_str, parse_kvp_from_command_args, parse_output_format,
};

pub(crate) fn workflow_command() -> Command {
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
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    short: None,
                    long: None,
                    value_type: ArgValueType::Enum(vec![
                        "validate", "lint", "preview", "graph", "run",
                        "resume", "runs", "checkpoint", "artifact", "import",
                    ]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: validate | lint | preview | graph | run | resume | runs | checkpoint | artifact",
                },
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
                ArgSpec {
                    name: "state-dir",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("state-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.",
                },
                ArgSpec {
                    name: "recursive",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("recursive"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Recursively walk workspace for all .newton/state/workflows directories (import)",
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
                        let context = parse_kvp_from_command_args(&args, "context")?;
                        let trigger = parse_kvp_from_command_args(&args, "trigger")?;
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
                                        state_dir: get_opt_path(&args, "state-dir"),
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
                        let mut a = args;
                        if !a.named.contains_key("workflow") {
                            if let Some(p) = a.named.get("subcommand2").cloned() {
                                a.named.insert("workflow".to_string(), p);
                            }
                        }
                        let run_args = RunArgs::try_from(a)?;
                        commands::workflow_run(run_args).await.map_err(anyhow::Error::from)
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
    }
}

pub(crate) fn webhook_command() -> Command {
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

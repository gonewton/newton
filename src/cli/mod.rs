//! CLI scaffolding for Newton: argument parsing, command definitions, and command dispatch logic.
pub mod args;
pub mod commands;
pub mod init;

pub use args::{
    ArtifactCommand, ArtifactsArgs, BatchArgs, CheckpointCommand, CheckpointsArgs, DotArgs,
    ExplainArgs, InitArgs, LintArgs, MonitorArgs, ResumeArgs, RunArgs, ServeArgs, ValidateArgs,
    WebhookArgs, WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use clap::{Parser, Subcommand};

const HELP_TEMPLATE: &str = "\
{name}\n\
{about-with-newline}\n\
USAGE:\n    {usage}\n\
\nOPTIONS:\n{options}\n\
WORKFLOW COMMANDS:\n{subcommands}\n";

#[derive(Parser)]
#[command(name = "newton")]
#[command(version = crate::VERSION)]
#[command(about = "Newton Loop optimization framework in Rust")]
#[command(help_template = HELP_TEMPLATE)]
#[command(
    after_long_help = "Typical flow: run an optimization, inspect status, emit reports, then debug errors if needed."
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // Command variants mirror the CLI styling structs, so matching variants stay large by design.
pub enum Command {
    #[command(
        about = "Execute a workflow graph",
        long_about = "Run executes a workflow graph defined in YAML, with optional trigger payload from input file or arguments.",
        after_help = "Example:\n    newton run workflow.yaml input.txt --workspace ./workspace --arg key=value"
    )]
    Run(RunArgs),
    #[command(
        about = "Initialize a Newton workspace with the default template",
        long_about = "Init creates the .newton workspace layout, installs the Newton template with aikit-sdk, and writes default configs so you can run immediately.",
        after_help = "Example:\n    newton init ./workspace"
    )]
    Init(InitArgs),
    #[command(
        about = "Process queued work items for a project",
        long_about = "Batch reads plan files from .newton/plan/<project_id> and drives headless workflow orchestration.",
        after_help = "Example:\n    newton batch project-alpha --workspace ./workspace"
    )]
    Batch(BatchArgs),
    #[command(
        about = "Start the Newton HTTP API server",
        long_about = "Serve starts the HTTP/WebSocket API server that provides real-time access to workflow execution state.

The API server exposes endpoints for:
    • Workflow instance management and queries
    • HIL (Human-in-the-Loop) event handling
    • Real-time streaming via WebSocket and SSE
    • Operator metadata and schema information

Use this command to run Newton as a backend service for web UIs, monitoring dashboards, or external integrations.

CORS is enabled for local development by default.",
        after_help = "EXAMPLES:
  Start API server on default port:
    newton serve

  Start on custom host and port:
    newton serve --host 0.0.0.0 --port 9000

  Run in background:
    newton serve --host 0.0.0.0 --port 8080 &

API ENDPOINTS:
    GET  /health              Health check endpoint
    GET  /api/workflows       List all workflow instances
    GET  /api/workflows/:id   Get workflow instance by ID
    PUT  /api/workflows/:id   Update workflow definition
    GET  /api/operators       List registered operators
    GET  /api/hil/workflows/:id            List HIL events for workflow
    POST /api/hil/workflows/:id/:eventId/action  Submit HIL action
    WS   /api/stream/workflow/:id/ws        WebSocket stream for workflow
    WS   /api/stream/logs/:id/:node_id/ws   WebSocket stream for node logs
    SSE  /api/stream/workflow/:id/sse      SSE stream for workflow events

LEGACY ENDPOINTS:
    GET  /api/channels        List workflow channels (legacy compatibility)"
    )]
    Serve(ServeArgs),
    #[command(
        about = "Monitor live ailoop channels via a terminal UI",
        long_about = "Monitor listens to every project/branch channel from the workspace using a WebSocket/HTTP mix and lets you answer questions or approve authorizations in a queue.\n\n\
CONFIGURATION:\n  \
Monitor requires both HTTP and WebSocket endpoints to connect to the ailoop server.\n  \
Endpoints can come from CLI overrides (--http-url, --ws-url) or workspace config files.\n  \
Partial overrides are supported: one flag can be set while the other comes from config.\n\n\
Endpoint discovery order:\n  \
  1. CLI overrides: --http-url and --ws-url (merged with config if partial)\n  \
  2. .newton/configs/monitor.conf (if present)\n  \
  3. First alphabetical .conf file in .newton/configs/ containing both keys\n\n\
Config files use simple key=value format:\n  \
  ailoop_server_http_url = http://127.0.0.1:8081\n  \
  ailoop_server_ws_url = ws://127.0.0.1:8080",
        after_help = "EXAMPLES:\n  \
Using both CLI overrides:\n    \
newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080\n\n  \
Using .newton/configs/monitor.conf:\n    \
newton monitor\n\n  \
Partial override (HTTP from CLI, WS from config):\n    \
newton monitor --http-url http://192.168.1.10:8081\n\n\
TROUBLESHOOTING:\n  \
Missing URL configuration:\n    \
If both endpoints are not found, ensure .newton/configs/monitor.conf exists\n    \
or provide both --http-url and --ws-url on the command line.\n\n  \
Connection refused / server unavailable:\n    \
Verify the ailoop server is running at the configured endpoints.\n    \
Check URLs use correct protocol schemes (http:// and ws://).\n\n  \
Missing .newton/configs workspace setup:\n    \
Run 'newton init' in your workspace root to create the .newton directory structure,\n    \
or manually create .newton/configs/ and add a monitor.conf file."
    )]
    Monitor(MonitorArgs),
    #[command(
        about = "Validate a workflow graph definition",
        long_about = "Validate checks your workflow YAML file for syntax errors, schema compliance, and logical issues before execution.\n\n\
This command performs comprehensive validation including:\n  \
  • YAML syntax and structure validation\n  \
  • Schema compliance checking\n  \
  • Task dependency validation\n  \
  • Resource and configuration verification\n\n\
Use validate before running workflows to catch errors early and ensure your workflow will execute successfully.",
        after_help = "EXAMPLES:\n  \
Validate a workflow file:\n    \
newton validate workflow.yaml\n\n  \
Validate with alternative syntax:\n    \
newton validate --file ./workflows/my-workflow.yaml\n\n\
RETURN CODES:\n  \
  0: Workflow is valid and ready to run\n  \
  1: Validation errors found (details printed to stderr)"
    )]
    Validate(ValidateArgs),
    #[command(
        about = "Generate a visual diagram of the workflow graph",
        long_about = "Dot creates a Graphviz DOT file from your workflow definition that can be rendered into visual diagrams.\n\n\
This command analyzes your workflow's task dependencies and generates a directed graph showing:\n  \
  • Task execution flow and dependencies\n  \
  • Parallel execution opportunities\n  \
  • Critical path through the workflow\n  \
  • Task relationships and data flow\n\n\
The output DOT file can be rendered to PNG, SVG, or PDF using Graphviz tools like 'dot' or online viewers.",
        after_help = "EXAMPLES:\n  \
Generate DOT file to stdout:\n    \
newton dot workflow.yaml\n\n  \
Save DOT file for rendering:\n    \
newton dot workflow.yaml --out graph.dot\n\n  \
Create PNG diagram (requires Graphviz):\n    \
newton dot workflow.yaml --out graph.dot && dot -Tpng graph.dot -o workflow.png\n\n\
VISUALIZATION:\n  \
Use online Graphviz viewers or install Graphviz locally:\n  \
  • Online: https://dreampuf.github.io/GraphvizOnline/\n  \
  • Install: apt install graphviz (Ubuntu) or brew install graphviz (macOS)"
    )]
    Dot(DotArgs),
    #[command(
        about = "Check workflow for best practices and potential issues",
        long_about = "Lint analyzes your workflow definition against Newton's best practices and coding standards to identify potential issues.\n\n\
This command performs static analysis checking for:\n  \
  • Performance anti-patterns\n  \
  • Resource usage optimization opportunities\n  \
  • Security considerations\n  \
  • Maintainability issues\n  \
  • Common workflow design mistakes\n\n\
Unlike validate (which checks syntax), lint focuses on quality and best practices. All lint warnings are advisory and won't prevent workflow execution.",
        after_help = "EXAMPLES:\n  \
Check workflow with human-readable output:\n    \
newton lint workflow.yaml\n\n  \
Generate JSON report for CI/CD integration:\n    \
newton lint workflow.yaml --format json\n\n  \
Lint with alternative file specification:\n    \
newton lint --file ./workflows/production.yaml --format json\n\n\
OUTPUT FORMATS:\n  \
  • text: Human-readable summary (default)\n  \
  • json: Machine-readable structured data for tooling integration"
    )]
    Lint(LintArgs),
    #[command(
        about = "Generate human-readable explanations of workflow behavior",
        long_about = "Explain creates detailed documentation about what your workflow does and how it will execute.\n\n\
This command analyzes your workflow definition and produces explanations covering:\n  \
  • Step-by-step execution flow\n  \
  • Task dependencies and timing\n  \
  • Configuration settings and their effects\n  \
  • Resource requirements and constraints\n  \
  • Expected inputs and outputs\n\n\
Use this command to understand complex workflows, document your automation, or verify that your workflow behaves as intended.",
        after_help = "EXAMPLES:\n  \
Generate structured explanation:\n    \
newton explain workflow.yaml --format text\n\n  \
Create natural language description:\n    \
newton explain workflow.yaml --format prose\n\n  \
Explain with custom trigger data:\n    \
newton explain workflow.yaml --arg env=production --format prose\n\n  \
Generate JSON explanation for documentation tools:\n    \
newton explain workflow.yaml --format json\n\n\
OUTPUT FORMATS:\n  \
  • text: Structured technical breakdown\n  \
  • prose: Natural language description\n  \
  • json: Machine-readable analysis for documentation generation"
    )]
    Explain(ExplainArgs),
    #[command(
        about = "Continue a workflow that was interrupted or stopped",
        long_about = "Resume restarts a workflow execution from its last saved checkpoint, allowing you to continue from where it left off.\n\n\
This command is useful when:\n  \
  • A workflow was interrupted by system shutdown or network issues\n  \
  • You need to modify execution parameters and continue\n  \
  • A long-running workflow needs to be restarted after maintenance\n  \
  • You want to debug a failed workflow by resuming from a specific point\n\n\
Newton automatically creates checkpoints during execution, so you can safely resume most workflows without losing progress.",
        after_help = "EXAMPLES:\n  \
Resume a specific workflow execution:\n    \
newton resume --execution-id 12345678-1234-1234-1234-123456789abc\n\n  \
Resume with custom workspace:\n    \
newton resume --execution-id abcdef01-2345-6789-abcd-ef0123456789 --workspace ./project\n\n  \
Resume and allow workflow definition changes:\n    \
newton resume --execution-id 12345678-1234-1234-1234-123456789abc --allow-workflow-change\n\n\
FINDING EXECUTION IDs:\n  \
List available executions to resume:\n    \
newton checkpoints list --workspace ./workspace\n\n\
SAFETY:\n  \
By default, resume requires the workflow definition to be unchanged since the checkpoint.\n  \
Use --allow-workflow-change to override this safety check if you've modified the workflow."
    )]
    Resume(ResumeArgs),
    #[command(
        about = "Manage and inspect workflow execution checkpoints",
        long_about = "Checkpoints provides tools to manage the saved states that allow workflow resumption after interruption.\n\n\
Newton automatically creates checkpoints during workflow execution to preserve progress and enable recovery. This command helps you:\n  \
  • View available executions that can be resumed\n  \
  • Clean up old checkpoint data to save disk space\n  \
  • Inspect checkpoint details for debugging\n  \
  • Monitor checkpoint storage usage\n\n\
Checkpoints include execution state, task progress, and all necessary context to safely resume workflows.",
        after_help = "EXAMPLES:\n  \
List all available checkpoints:\n    \
newton checkpoints list --workspace ./workspace\n\n  \
Get checkpoint details in JSON format:\n    \
newton checkpoints list --workspace ./workspace --format-json\n\n  \
Clean old checkpoints (older than 7 days):\n    \
newton checkpoints clean --workspace ./workspace --older-than 7d\n\n  \
Clean checkpoints with custom retention:\n    \
newton checkpoints clean --workspace ./workspace --older-than 30d\n\n\
CHECKPOINT STORAGE:\n  \
Checkpoints are stored in .newton/checkpoints/ within your workspace.\n  \
Large workflows may generate substantial checkpoint data over time."
    )]
    Checkpoints(CheckpointsArgs),
    #[command(
        about = "Manage workflow output files and execution artifacts",
        long_about = "Artifacts provides tools to manage the files, logs, and output data generated during workflow execution.\n\n\
Newton stores workflow outputs, logs, and temporary files as artifacts for debugging and analysis. This command helps you:\n  \
  • Clean up old artifacts to reclaim disk space\n  \
  • Manage artifact retention policies\n  \
  • Monitor artifact storage usage\n  \
  • Archive important execution results\n\n\
Artifacts include task outputs, execution logs, intermediate files, and any data generated by your workflow tasks.",
        after_help = "EXAMPLES:\n  \
Clean artifacts older than 7 days:\n    \
newton artifacts clean --workspace ./workspace --older-than 7d\n\n  \
Clean with custom retention period:\n    \
newton artifacts clean --workspace ./workspace --older-than 30d\n\n  \
Clean artifacts in specific workspace:\n    \
newton artifacts clean --workspace /path/to/project --older-than 1w\n\n\
RETENTION FORMATS:\n  \
Supported time formats for --older-than:\n  \
  • Days: 7d, 30d\n  \
  • Weeks: 1w, 2w\n  \
  • Hours: 24h, 48h\n\n\
ARTIFACT STORAGE:\n  \
Artifacts are stored in .newton/artifacts/ within your workspace.\n  \
Regular cleanup helps maintain good performance and disk usage."
    )]
    Artifacts(ArtifactsArgs),
    #[command(
        about = "Run webhooks to trigger workflows from external events",
        long_about = "Webhook provides HTTP endpoints that can trigger workflow executions in response to external events.\n\n\
This command enables integration with:\n  \
  • Git hosting services (GitHub, GitLab, Bitbucket)\n  \
  • CI/CD platforms and build systems\n  \
  • Monitoring and alerting systems\n  \
  • Custom applications and services\n\n\
Webhooks allow you to automate workflow execution based on external triggers, creating reactive automation pipelines.",
        after_help = "EXAMPLES:\n  \
Start webhook server for a workflow:\n    \
newton webhook serve workflow.yaml --workspace ./workspace\n\n  \
Check webhook configuration status:\n    \
newton webhook status workflow.yaml --workspace ./workspace\n\n  \
Serve webhook with alternative file syntax:\n    \
newton webhook serve --file ./workflows/deploy.yaml --workspace ./project\n\n\
INTEGRATION:\n  \
Configure your external services to send POST requests to the webhook URL.\n  \
The webhook server will parse the incoming payload and trigger the workflow with the event data.\n\n\
SECURITY:\n  \
Webhook endpoints include built-in security features like request validation and rate limiting.\n  \
Configure authentication tokens and HTTPS for production deployments."
    )]
    Webhook(WebhookArgs),
}

pub async fn run(args: Args) -> crate::Result<()> {
    match args.command {
        Command::Run(run_args) => commands::run(run_args).await,
        Command::Init(init_args) => init::run(init_args).await,
        Command::Batch(batch_args) => commands::batch(batch_args).await,
        Command::Serve(serve_args) => commands::serve(serve_args)
            .await
            .map_err(anyhow::Error::from),
        Command::Monitor(monitor_args) => commands::monitor(monitor_args).await,
        Command::Validate(validate_args) => {
            commands::validate(validate_args).map_err(anyhow::Error::from)
        }
        Command::Dot(dot_args) => commands::dot(dot_args).map_err(anyhow::Error::from),
        Command::Lint(lint_args) => commands::lint(lint_args).map_err(anyhow::Error::from),
        Command::Explain(explain_args) => {
            commands::explain(explain_args).map_err(anyhow::Error::from)
        }
        Command::Resume(resume_args) => commands::resume(resume_args)
            .await
            .map_err(anyhow::Error::from),
        Command::Checkpoints(checkpoints_args) => {
            commands::checkpoints(checkpoints_args).map_err(anyhow::Error::from)
        }
        Command::Artifacts(artifacts_args) => {
            commands::artifacts(artifacts_args).map_err(anyhow::Error::from)
        }
        Command::Webhook(webhook_args) => commands::webhook(webhook_args)
            .await
            .map_err(anyhow::Error::from),
    }
}

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

pub(super) const INIT_LONG_ABOUT: &str = "\
Init creates the .newton workspace layout, installs the Newton template with \
aikit-sdk, and writes default configs so you can run immediately.

EXAMPLES:
  Initialize current directory:
    newton init .

  Initialize a specific directory:
    newton init ./workspace

  Initialize with custom template source:
    newton init . --template gonewton/newton-templates";

pub(super) const OPTIMIZE_LONG_ABOUT: &str = "\
Optimize reads Plans from .newton/plan/<project_id>/todo and drives the \
autonomous optimization loop until the Plan queue is drained.

EXAMPLES:
  Drive the optimization loop for a project:
    newton optimize project-alpha

  With workspace override:
    newton optimize project-alpha --workspace ./workspace

  Process one Plan and exit:
    newton optimize project-alpha --once

  Custom poll interval (seconds):
    newton optimize project-alpha --poll-interval 30";

pub(super) const SERVE_LONG_ABOUT: &str = "\
Serve runs the Newton HTTP/WebSocket API for UIs, agents, and integrations.
Full REST contract: openapi/newton-api.yaml.

EXAMPLES:
  Start API server on default port:
    newton serve

  Start on custom host and port:
    newton serve --host 0.0.0.0 --port 9000

  Start API-only (no embedded web UI):
    newton serve --no-web";

pub(super) const WORKFLOW_LONG_ABOUT: &str = "\
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

pub(super) const DATA_GET_LONG_ABOUT: &str =
    "Retrieve catalog entities — either a full collection or a single item by id.\n\n\
     EXAMPLES:\n  \
     newton data get products\n  \
     newton data get product <id> --json\n  \
     newton data get kpis\n  \
     newton data get kpi <id> --json\n  \
     newton data get eval-runs\n  \
     newton data get eval-runs --scope repo --scope-id gonewton-newton\n  \
     newton data get eval-runs --source dk-review --limit 25\n  \
     newton data get eval-run <id> --json\n  \
     newton data get grades\n  \
     newton data get grades --run-id <runId>\n  \
     newton data get grades --kpi-id <kpiId>\n  \
     newton data get grade <id>\n  \
     newton data get findings --status triaged\n  \
     newton data get findings --scope component --scope-id auth-service\n  \
     newton data get change-requests --status proposed\n  \
     newton data get plans --status ready\n  \
     newton data get optimize-cycles --run-id <runId>\n  \
     newton data get optimize-cycle <cycleId> --run-id <runId>";

pub(super) const DATA_POST_LONG_ABOUT: &str =
    "Create a new catalog entity. For EvalRun and Grade, the caller MUST provide a stable `id`.\n\n\
     EXAMPLES:\n  \
     newton data post product -f body.json\n  \
     newton data post component -f body.json --dry-run\n  \
     newton data post eval-run -f evalrun.json\n  \
     newton data post grade -f grade.json";

pub(super) const DATA_PUT_LONG_ABOUT: &str =
    "Replace an existing catalog entity (full update).  The entity id is required.\n\n\
     EXAMPLES:\n  \
     newton data put product <id> -f body.json";

pub(super) const DATA_PATCH_LONG_ABOUT: &str =
    "Partially update an existing catalog entity.  The entity id is required.\n\n\
     EXAMPLES:\n  \
     newton data patch product <id> --body '{\"name\":\"X\"}'";

pub(super) const DATA_DELETE_LONG_ABOUT: &str = "Delete a catalog entity by id.\n\n\
     DELETE is only implemented for: product, component, repo, module, \
     module-dependency. The remaining resources are lifecycle-managed \
     (retired via `newton data patch <resource> <id> --body '{\"status\":...}'`) \
     or append-only historical records, and reject DELETE with a message \
     naming the correct operation instead of a generic error:\n  \
     - finding: PATCH status to `rejected`/`deferred` (or let it auto-resolve/auto-block)\n  \
     - change-request: PATCH status to `rejected`\n  \
     - plan: PATCH status to `abandoned`\n  \
     - optimize-run: PATCH status if it needs to be force-closed\n  \
     - optimize-cycle: immutable Trajectory entry, never deletable\n  \
     - kpi, eval-run, grade: append-only catalog/history, never deletable\n\n\
     EXAMPLES:\n  \
     newton data delete product <id>\n  \
     newton data patch finding <id> --body '{\"status\":\"rejected\"}'";

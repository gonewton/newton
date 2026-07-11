/**
 * Operator constructors — Layer 2 thin wrappers.
 *
 * Each constructor returns an OperatorCall with an operatorType key
 * that compile.ts uses to emit the correct YAML operator name and params.
 */

import type { Ref, Guard } from "./refs.js";
import { OUTPUT_SCHEMAS as _OUTPUT_SCHEMAS } from "./generated/output_schemas.js";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyValue = unknown;

function renderValue(v: AnyValue): AnyValue {
  if (v instanceof Object && "toCondition" in v && typeof (v as { toCondition: unknown }).toCondition === "function") {
    return (v as { toCondition: () => unknown }).toCondition();
  }
  if (Array.isArray(v)) return v.map(renderValue);
  if (v !== null && typeof v === "object") {
    return Object.fromEntries(
      Object.entries(v as Record<string, AnyValue>).map(([k, val]) => [k, renderValue(val)])
    );
  }
  return v;
}

export class OperatorCall {
  // Generated from `newton schema export --outputs` — do not edit by hand.
  static readonly OUTPUT_SCHEMAS: Record<string, string[]> = _OUTPUT_SCHEMAS;

  constructor(
    public readonly operatorType: string,
    public readonly params: Record<string, AnyValue>
  ) {}

  renderedParams(): Record<string, AnyValue> {
    return renderValue(this.params) as Record<string, AnyValue>;
  }

  outputFields(): string[] | null {
    return OperatorCall.OUTPUT_SCHEMAS[this.operatorType] ?? null;
  }
}

// --------------------------------------------------------------------------
// CommandOperator
// --------------------------------------------------------------------------

export interface CommandOpts {
  cmd: string;
  shell?: boolean;
  captureStdout?: boolean;
  captureStderr?: boolean;
  env?: Record<string, string | Ref | Guard>;
  cwd?: string | Ref;
  writeStdout?: string;
  writeStderr?: string;
}

export function command(opts: CommandOpts): OperatorCall {
  const params: Record<string, AnyValue> = { cmd: opts.cmd };
  if (opts.shell) params.shell = true;
  if (opts.captureStdout) params.capture_stdout = true;
  if (opts.captureStderr) params.capture_stderr = true;
  if (opts.env) params.env = opts.env;
  if (opts.cwd != null) params.cwd = opts.cwd;
  if (opts.writeStdout != null) params.write_stdout = opts.writeStdout;
  if (opts.writeStderr != null) params.write_stderr = opts.writeStderr;
  return new OperatorCall("CommandOperator", params);
}

// --------------------------------------------------------------------------
// AgentOperator
// --------------------------------------------------------------------------

export interface AgentOpts {
  engine?: AnyValue;
  model?: AnyValue;
  prompt?: AnyValue;
  promptFile?: string;
  signals?: Record<string, string>;
  requireSignal?: boolean;
  streamStdout?: boolean;
  contextFidelity?: string;
}

export function agent(opts: AgentOpts = {}): OperatorCall {
  const params: Record<string, AnyValue> = {};
  if (opts.engine != null) params.engine = opts.engine;
  if (opts.model != null) params.model = opts.model;
  if (opts.prompt != null) params.prompt = opts.prompt;
  if (opts.promptFile != null) params.prompt_file = opts.promptFile;
  if (opts.signals != null) params.signals = opts.signals;
  if (opts.requireSignal) params.require_signal = true;
  if (opts.streamStdout != null) params.stream_stdout = opts.streamStdout;
  if (opts.contextFidelity != null) params.context_fidelity = opts.contextFidelity;
  return new OperatorCall("AgentOperator", params);
}

// --------------------------------------------------------------------------
// HumanApprovalOperator
// --------------------------------------------------------------------------

export interface HumanApprovalOpts {
  prompt: string;
  timeoutSeconds?: number;
  defaultOnTimeout?: string;
}

export function humanApproval(opts: HumanApprovalOpts): OperatorCall {
  const params: Record<string, AnyValue> = { prompt: opts.prompt };
  if (opts.timeoutSeconds != null) params.timeout_seconds = opts.timeoutSeconds;
  if (opts.defaultOnTimeout != null) params.default_on_timeout = opts.defaultOnTimeout;
  return new OperatorCall("HumanApprovalOperator", params);
}

// --------------------------------------------------------------------------
// HumanDecisionOperator
// --------------------------------------------------------------------------

export interface HumanDecisionOption {
  label: string;
  description?: string;
  recommendation?: boolean;
}

export interface HumanDecisionOpts {
  /** Structured form: list of options with labels. */
  options?: HumanDecisionOption[];
  /** Legacy form: freeform prompt with explicit choices. */
  prompt?: string;
  choices?: string[];
  timeoutSeconds?: number;
  defaultChoice?: string;
}

export function humanDecision(opts: HumanDecisionOpts): OperatorCall {
  const params: Record<string, AnyValue> = {};
  if (opts.options != null) params.options = opts.options;
  if (opts.prompt != null) params.prompt = opts.prompt;
  if (opts.choices != null) params.choices = opts.choices;
  if (opts.timeoutSeconds != null) params.timeout_seconds = opts.timeoutSeconds;
  if (opts.defaultChoice != null) params.default_choice = opts.defaultChoice;
  return new OperatorCall("HumanDecisionOperator", params);
}

// --------------------------------------------------------------------------
// WorkflowOperator
// --------------------------------------------------------------------------

export interface SubWorkflowOpts {
  workflowPath: AnyValue;
  triggers?: Record<string, AnyValue>;
  context?: Record<string, AnyValue>;
}

export function subWorkflow(opts: SubWorkflowOpts): OperatorCall {
  const params: Record<string, AnyValue> = { workflow_path: opts.workflowPath };
  if (opts.triggers) params.triggers = opts.triggers;
  if (opts.context) params.context = opts.context;
  return new OperatorCall("WorkflowOperator", params);
}

// --------------------------------------------------------------------------
// barrier
// --------------------------------------------------------------------------

export interface BarrierOpts {
  /** Task ids that must all complete before this barrier passes. */
  expected?: string[];
}

export function barrier(opts: BarrierOpts = {}): OperatorCall {
  const params: Record<string, AnyValue> = {};
  if (opts.expected != null) params.expected = opts.expected;
  return new OperatorCall("barrier", params);
}

// --------------------------------------------------------------------------
// SetContextOperator
// --------------------------------------------------------------------------

export interface SetContextOpts {
  /** JSON-merge patch applied to the workflow context. */
  patch: Record<string, AnyValue>;
}

export function setContext(opts: SetContextOpts): OperatorCall {
  return new OperatorCall("SetContextOperator", { patch: opts.patch });
}

// --------------------------------------------------------------------------
// NoOpOperator
// --------------------------------------------------------------------------

export function noop(): OperatorCall {
  return new OperatorCall("NoOpOperator", {});
}

// --------------------------------------------------------------------------
// GraderCommandOperator — spec 062. Runs a shell command Grader.
// --------------------------------------------------------------------------

export interface GraderCommandOpts {
  cmd: string;
  grader: string;
  scope: AnyValue;
  scopeId: AnyValue;
  shell?: string;
  cwd?: string | Ref;
  timeoutSeconds?: number;
  env?: Record<string, string | Ref | Guard>;
  state?: Record<string, string | Ref | Guard>;
}

export function graderCommand(opts: GraderCommandOpts): OperatorCall {
  const params: Record<string, AnyValue> = {
    cmd: opts.cmd,
    grader: opts.grader,
    scope: opts.scope,
    scope_id: opts.scopeId,
  };
  if (opts.shell != null) params.shell = opts.shell;
  if (opts.cwd != null) params.cwd = opts.cwd;
  if (opts.timeoutSeconds != null) params.timeout_seconds = opts.timeoutSeconds;
  if (opts.env != null) params.env = opts.env;
  if (opts.state != null) params.state = opts.state;
  return new OperatorCall("GraderCommandOperator", params);
}

// --------------------------------------------------------------------------
// ReconcileOperator — spec 063 + 067. Reconciles Observations into Findings.
// --------------------------------------------------------------------------

export interface ReconcileOpts {
  scope: AnyValue;
  scopeId: AnyValue;
  /** Assessment JSON — typically bound via a prior grader task's output. */
  assessment: AnyValue;
  grader?: string;
  engine?: AnyValue;
  model?: AnyValue;
  adjudicationTimeoutSeconds?: number;
}

export function reconcile(opts: ReconcileOpts): OperatorCall {
  const params: Record<string, AnyValue> = {
    scope: opts.scope,
    scope_id: opts.scopeId,
    assessment: opts.assessment,
  };
  if (opts.grader != null) params.grader = opts.grader;
  if (opts.engine != null) params.engine = opts.engine;
  if (opts.model != null) params.model = opts.model;
  if (opts.adjudicationTimeoutSeconds != null) {
    params.adjudication_timeout_seconds = opts.adjudicationTimeoutSeconds;
  }
  return new OperatorCall("ReconcileOperator", params);
}

// --------------------------------------------------------------------------
// ChangeRequestOperator — spec 064 + 067. Synthesizes a ChangeRequest from
// open Findings.
// --------------------------------------------------------------------------

export interface ChangeRequestOpts {
  scope: AnyValue;
  scopeId: AnyValue;
  maxFindings?: number;
  minSeverity?: string;
  engine?: AnyValue;
  model?: AnyValue;
  synthesisTimeoutSeconds?: number;
}

export function changeRequest(opts: ChangeRequestOpts): OperatorCall {
  const params: Record<string, AnyValue> = {
    scope: opts.scope,
    scope_id: opts.scopeId,
  };
  if (opts.maxFindings != null) params.max_findings = opts.maxFindings;
  if (opts.minSeverity != null) params.min_severity = opts.minSeverity;
  if (opts.engine != null) params.engine = opts.engine;
  if (opts.model != null) params.model = opts.model;
  if (opts.synthesisTimeoutSeconds != null) {
    params.synthesis_timeout_seconds = opts.synthesisTimeoutSeconds;
  }
  return new OperatorCall("ChangeRequestOperator", params);
}

// --------------------------------------------------------------------------
// GraderAgentOperator — spec 065 + 067. Rubric-based AI grader.
// --------------------------------------------------------------------------

export interface GraderAgentOpts {
  grader: string;
  scope: AnyValue;
  scopeId: AnyValue;
  rubric: AnyValue;
  model?: AnyValue;
  engine?: AnyValue;
  timeoutSeconds?: number;
}

export function graderAgent(opts: GraderAgentOpts): OperatorCall {
  const params: Record<string, AnyValue> = {
    grader: opts.grader,
    scope: opts.scope,
    scope_id: opts.scopeId,
    rubric: opts.rubric,
  };
  if (opts.model != null) params.model = opts.model;
  if (opts.engine != null) params.engine = opts.engine;
  if (opts.timeoutSeconds != null) params.timeout_seconds = opts.timeoutSeconds;
  return new OperatorCall("GraderAgentOperator", params);
}

// --------------------------------------------------------------------------
// GhOperator sub-constructors
// --------------------------------------------------------------------------

export const gh = {
  prCreate(opts: {
    base: string;
    title: AnyValue;
    body: string;
    retryCount?: number;
    retryDelayMs?: number;
    draft?: boolean;
  }): OperatorCall {
    const params: Record<string, AnyValue> = {
      operation: "pr_create",
      base: opts.base,
      title: opts.title,
      body: opts.body,
    };
    if (opts.retryCount != null) params.retry_count = opts.retryCount;
    if (opts.retryDelayMs != null) params.retry_delay_ms = opts.retryDelayMs;
    if (opts.draft != null) params.draft = opts.draft;
    return new OperatorCall("GhOperator", params);
  },

  prView(opts: { pr: AnyValue }): OperatorCall {
    return new OperatorCall("GhOperator", { operation: "pr_view", pr: opts.pr });
  },

  prApprove(opts: { prNumber: AnyValue }): OperatorCall {
    return new OperatorCall("GhOperator", { operation: "pr_approve", pr_number: opts.prNumber });
  },

  projectResolveBoard(opts: {
    owner: AnyValue;
    projectNumber?: AnyValue;
    requiredOptionNames?: string[];
  }): OperatorCall {
    const params: Record<string, AnyValue> = {
      operation: "project_resolve_board",
      owner: opts.owner,
    };
    if (opts.projectNumber != null) params.project_number = opts.projectNumber;
    if (opts.requiredOptionNames != null) params.required_option_names = opts.requiredOptionNames;
    return new OperatorCall("GhOperator", params);
  },

  projectItemSetStatus(opts: {
    itemId: AnyValue;
    board: AnyValue;
    status: string;
    onError?: string;
  }): OperatorCall {
    const params: Record<string, AnyValue> = {
      operation: "project_item_set_status",
      item_id: opts.itemId,
      board: opts.board,
      status: opts.status,
    };
    if (opts.onError != null) params.on_error = opts.onError;
    return new OperatorCall("GhOperator", params);
  },
};

// --------------------------------------------------------------------------
// GitOperator sub-constructors
// --------------------------------------------------------------------------

export const git = {
  cleanCheck(): OperatorCall {
    return new OperatorCall("GitOperator", { operation: "clean_check" });
  },

  syncMain(): OperatorCall {
    return new OperatorCall("GitOperator", { operation: "sync_main" });
  },

  createBranch(opts: { name: AnyValue }): OperatorCall {
    return new OperatorCall("GitOperator", { operation: "create_branch", name: opts.name });
  },

  stage(opts: { exclude?: string[] } = {}): OperatorCall {
    const params: Record<string, AnyValue> = { operation: "stage" };
    if (opts.exclude?.length) params.exclude = opts.exclude;
    return new OperatorCall("GitOperator", params);
  },

  commit(opts: { message: AnyValue; allowEmpty?: boolean }): OperatorCall {
    const params: Record<string, AnyValue> = {
      operation: "commit",
      message: opts.message,
    };
    if (opts.allowEmpty) params.allow_empty = true;
    return new OperatorCall("GitOperator", params);
  },

  push(opts: {
    remote?: string;
    force?: boolean;
    retryCount?: number;
    retryDelayMs?: number;
  } = {}): OperatorCall {
    return new OperatorCall("GitOperator", {
      operation: "push",
      remote: opts.remote ?? "origin",
      force: opts.force ?? false,
      retry_count: opts.retryCount ?? 3,
      retry_delay_ms: opts.retryDelayMs ?? 5000,
    });
  },

  diff(opts: { base?: string; maxBytes?: number } = {}): OperatorCall {
    return new OperatorCall("GitOperator", {
      operation: "diff",
      base: opts.base ?? "main",
      max_bytes: opts.maxBytes ?? 65536,
    });
  },

  cleanupMerge(): OperatorCall {
    return new OperatorCall("GitOperator", { operation: "cleanup_merge" });
  },
};

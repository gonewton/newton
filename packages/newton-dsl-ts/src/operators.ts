/**
 * Operator constructors — Layer 2 thin wrappers.
 *
 * Each constructor returns an OperatorCall with an operatorType key
 * that compile.ts uses to emit the correct YAML operator name and params.
 */

import type { Ref, Guard } from "./refs.js";

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
  // Known output schemas per operator type — used by checks.ts for .out.field validation
  static readonly OUTPUT_SCHEMAS: Record<string, string[]> = {
    CommandOperator: ["stdout", "stderr", "exit_code", "stdout_artifact", "stderr_artifact"],
    AgentOperator: ["stdout", "stderr", "signal", "exit_code", "stdout_artifact", "stderr_artifact"],
    GhOperator: ["pr_number", "pr_url", "state", "merged", "number", "title"],
    HumanOperator: ["response", "choice", "timed_out"],
    WorkflowOperator: ["output", "status"],
  };

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
// HumanOperator
// --------------------------------------------------------------------------

export interface HumanApprovalOpts {
  ask: string;
  timeoutSeconds?: number;
  defaultOnTimeout?: string;
}

export function humanApproval(opts: HumanApprovalOpts): OperatorCall {
  const params: Record<string, AnyValue> = {
    ask: opts.ask,
    timeout_seconds: opts.timeoutSeconds ?? 300,
    default_on_timeout: opts.defaultOnTimeout ?? "timeout",
  };
  return new OperatorCall("HumanOperator", params);
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

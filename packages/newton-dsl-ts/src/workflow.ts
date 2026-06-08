/**
 * Workflow class — top-level builder for a workflow graph.
 *
 * Usage:
 *   const wf = new Workflow("my-workflow", { defaultEngine: "codex" });
 *   const t1 = wf.task("step1", command({ cmd: "echo hello", shell: true }));
 *   const t2 = wf.finish("done");
 *   t1.then(t2);
 *   console.log(wf.toYaml());
 */

import { Task } from "./task.js";
import { command } from "./operators.js";
import type { OperatorCall } from "./operators.js";
import {
  makeInputProxy,
  makeContextProxy,
  EnvRef,
  AmbientRef,
} from "./refs.js";
import type { ScopeProxy } from "./refs.js";
import { compileWorkflow } from "./compile.js";

export interface WorkflowOpts {
  defaultEngine?: string;
  parallelLimit?: number;
  maxTimeSeconds?: number;
  entryTask?: string;
  maxTaskIterations?: number;
  maxWorkflowIterations?: number;
  continueOnError?: boolean;
  description?: string;
  allowShell?: boolean;
  artifactStorage?: Record<string, unknown>;
}

export class Workflow {
  public readonly _name: string;
  public _defaultEngine: string | null;
  public _parallelLimit: number;
  public _maxTimeSeconds: number;
  public _entryTask: string | null;
  public _maxTaskIterations: number | null;
  public _maxWorkflowIterations: number | null;
  public _continueOnError: boolean;
  public _description: string | null;
  public _allowShell: boolean;
  public _artifactStorage: Record<string, unknown> | null;
  public _commandOperatorSettings: { allow_shell: boolean } | null;

  public _tasks: Map<string, Task>;
  public _taskList: Task[];
  public _context: Record<string, unknown>;
  public _triggerPayload: Record<string, unknown>;
  public _expectedVars: string[];

  public _metadata: Record<string, unknown>;
  public _triggers: Record<string, unknown>;

  constructor(name: string, opts: WorkflowOpts = {}) {
    this._name = name;
    this._defaultEngine = opts.defaultEngine ?? null;
    this._parallelLimit = opts.parallelLimit ?? 1;
    this._maxTimeSeconds = opts.maxTimeSeconds ?? 3600;
    this._entryTask = opts.entryTask ?? null;
    this._maxTaskIterations = opts.maxTaskIterations ?? null;
    this._maxWorkflowIterations = opts.maxWorkflowIterations ?? null;
    this._continueOnError = opts.continueOnError ?? false;
    this._description = opts.description ?? null;
    this._allowShell = opts.allowShell ?? false;
    this._artifactStorage = opts.artifactStorage ?? null;
    this._commandOperatorSettings = opts.allowShell ? { allow_shell: true } : null;

    this._tasks = new Map();
    this._taskList = [];
    this._context = {};
    this._triggerPayload = {};
    this._expectedVars = [];

    this._metadata = { name };
    if (opts.description) this._metadata.description = opts.description;

    this._triggers = {
      type: "manual",
      schema_version: "1.0",
      payload: {},
    };
  }

  // -------------------------------------------------------------------------
  // Configuration
  // -------------------------------------------------------------------------

  inputs(defaults: Record<string, string>): this {
    Object.assign(this._triggerPayload, defaults);
    this._triggers.payload = this._triggerPayload;
    return this;
  }

  expects(...names: string[]): this {
    this._expectedVars.push(...names);
    return this;
  }

  setContext(ctx: Record<string, unknown>): this {
    Object.assign(this._context, ctx);
    return this;
  }

  // -------------------------------------------------------------------------
  // Typed reference factories
  // -------------------------------------------------------------------------

  get input(): ScopeProxy {
    return makeInputProxy();
  }

  get var(): ScopeProxy {
    return makeContextProxy();
  }

  get context(): ScopeProxy {
    return makeContextProxy();
  }

  env(name: string): EnvRef {
    return new EnvRef(name);
  }

  ambient(name: string): AmbientRef {
    return new AmbientRef(name);
  }

  // -------------------------------------------------------------------------
  // Task registration
  // -------------------------------------------------------------------------

  task(id: string, operatorCall: OperatorCall, opts?: { name?: string }): Task {
    if (this._tasks.has(id)) {
      throw new Error(`Task '${id}' already registered in workflow '${this._name}'`);
    }
    const t = new Task(id, operatorCall, opts);
    this._tasks.set(id, t);
    this._taskList.push(t);

    if (this._entryTask == null) {
      this._entryTask = id;
    }
    return t;
  }

  finish(id: string, opts?: { message?: string }): Task {
    const msg = opts?.message ?? `Workflow ${this._name} completed successfully.`;
    const op = command({ cmd: `echo ${JSON.stringify(msg)}`, shell: true });
    const t = this.task(id, op);
    t._terminal = "success";
    t._message = opts?.message ?? null;
    return t;
  }

  fail(id: string, opts?: { message?: string }): Task {
    const msg = opts?.message ?? `Workflow ${this._name} failed.`;
    const op = command({ cmd: `echo ${JSON.stringify(msg)} >&2; exit 1`, shell: true });
    const t = this.task(id, op);
    t._terminal = "failure";
    t._message = opts?.message ?? null;
    return t;
  }

  // -------------------------------------------------------------------------
  // Serialization
  // -------------------------------------------------------------------------

  toYaml(): string {
    return compileWorkflow(this);
  }

  compile(): string {
    return this.toYaml();
  }
}

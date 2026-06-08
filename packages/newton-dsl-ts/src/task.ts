/**
 * Task handle returned from wf.task(...).
 *
 * Supports chaining: task.then(...).then(...).retry(...)
 */

import { EdgeSpec, PRIORITY_STEP } from "./edges.js";
import { Guard, Ref, makeOutProxy, TaskOutputRef } from "./refs.js";
import type { OutProxy } from "./refs.js";
import type { OperatorCall } from "./operators.js";

export interface RetryOpts {
  times: number;
  waitSeconds?: number;
  multiplier?: number;
  jitterSeconds?: number;
}

export interface ThenOpts {
  when?: Guard | Ref | string;
  label?: string;
}

export class Task {
  public readonly taskId: string;
  public readonly operatorCall: OperatorCall;
  public name: string | null = null;
  public _edges: EdgeSpec[] = [];
  public _nextPriority: number = 0;
  public _retry: Record<string, unknown> | null = null;
  public _maxIterations: number | null = null;
  public _timeoutMs: number | null = null;
  public _terminal: string | null = null;
  public _message: string | null = null;

  constructor(taskId: string, operatorCall: OperatorCall, opts?: { name?: string }) {
    this.taskId = taskId;
    this.operatorCall = operatorCall;
    if (opts?.name) this.name = opts.name;
  }

  // -------------------------------------------------------------------------
  // Transition wiring
  // -------------------------------------------------------------------------

  then(target: Task, opts?: ThenOpts): this {
    let guard: Guard | null = null;
    const w = opts?.when;
    if (w != null) {
      if (w instanceof Guard) {
        guard = w;
      } else if (w instanceof Ref) {
        guard = new Guard(w.rhaiExpr());
      } else {
        guard = new Guard(String(w));
      }
    }

    const edge = new EdgeSpec(
      target.taskId,
      this._nextPriority,
      guard,
      opts?.label ?? null
    );
    this._edges.push(edge);
    this._nextPriority += PRIORITY_STEP;
    return this;
  }

  otherwise(target: Task, opts?: { label?: string }): this {
    const edge = new EdgeSpec(
      target.taskId,
      this._nextPriority,
      null,
      opts?.label ?? null
    );
    this._edges.push(edge);
    this._nextPriority += PRIORITY_STEP;
    return this;
  }

  // -------------------------------------------------------------------------
  // Task configuration
  // -------------------------------------------------------------------------

  retry(opts: RetryOpts): this {
    const r: Record<string, unknown> = {
      max_attempts: opts.times,
      backoff_ms: (opts.waitSeconds ?? 0) * 1000,
    };
    if (opts.multiplier != null) r.backoff_multiplier = opts.multiplier;
    if (opts.jitterSeconds != null) r.jitter_ms = opts.jitterSeconds * 1000;
    this._retry = r;
    return this;
  }

  repeatAtMost(n: number): this {
    this._maxIterations = n;
    return this;
  }

  timeout(seconds: number): this {
    this._timeoutMs = seconds * 1000;
    return this;
  }

  // -------------------------------------------------------------------------
  // Output references
  // -------------------------------------------------------------------------

  get out(): OutProxy {
    return makeOutProxy(this.taskId);
  }

  get output(): TaskOutputRef {
    return new TaskOutputRef(this.taskId);
  }

  // -------------------------------------------------------------------------
  // Serialization
  // -------------------------------------------------------------------------

  toDict(): Record<string, unknown> {
    const d: Record<string, unknown> = {
      id: this.taskId,
      operator: this.operatorCall.operatorType,
    };
    if (this.name != null) d.name = this.name;
    if (this._maxIterations != null) d.max_iterations = this._maxIterations;
    if (this._timeoutMs != null) d.timeout_ms = this._timeoutMs;
    if (this._retry != null) d.retry = this._retry;
    if (this._terminal != null) d.terminal = this._terminal;

    d.params = this.operatorCall.renderedParams();
    d.transitions = this._edges.map((e) => e.toDict());
    return d;
  }
}

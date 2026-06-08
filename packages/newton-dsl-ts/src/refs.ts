/**
 * Typed reference helpers that render to Rhai $expr values in the compiled YAML.
 *
 * - t.out.field       -> OutRef("tasks.<id>.output.<field>")
 * - wf.input.x        -> InputRef("x")      renders as {"$expr": "triggers.x"}
 * - wf.context.x      -> ContextRef("x")    renders as {"$expr": "context.x"}
 * - wf.var.x          -> ContextRef("x")    (alias for context)
 * - wf.env("VAR")     -> EnvRef("VAR")      renders as {"$expr": 'env("VAR")'}
 * - ref.eq(value)     -> Guard (Rhai comparison expression)
 */

function rhaiLiteral(value: unknown): string {
  if (typeof value === "string") return `"${value}"`;
  if (typeof value === "boolean") return value ? "true" : "false";
  if (value === null || value === undefined) return "null";
  return String(value);
}

export interface ExprDict {
  $expr: string;
}

/**
 * Base class for all typed references.
 */
export abstract class Ref {
  abstract rhaiExpr(): string;

  toCondition(): ExprDict {
    return { $expr: this.rhaiExpr() };
  }

  eq(other: unknown): Guard {
    return new Guard(`${this.rhaiExpr()} == ${rhaiLiteral(other)}`);
  }

  ne(other: unknown): Guard {
    return new Guard(`${this.rhaiExpr()} != ${rhaiLiteral(other)}`);
  }

  gt(other: unknown): Guard {
    return new Guard(`${this.rhaiExpr()} > ${rhaiLiteral(other)}`);
  }

  lt(other: unknown): Guard {
    return new Guard(`${this.rhaiExpr()} < ${rhaiLiteral(other)}`);
  }
}

/**
 * A boolean-valued Rhai expression used as a transition condition.
 */
export class Guard {
  private readonly _expr: string;

  constructor(expr: string) {
    this._expr = expr;
  }

  rhaiExpr(): string {
    return this._expr;
  }

  toCondition(): ExprDict {
    return { $expr: this._expr };
  }

  and(other: Guard): Guard {
    return new Guard(`(${this._expr}) && (${other._expr})`);
  }

  or(other: Guard): Guard {
    return new Guard(`(${this._expr}) || (${other._expr})`);
  }
}

/**
 * Reference to a task output field: tasks.<id>.output.<field>
 */
export class OutRef extends Ref {
  constructor(
    private readonly _taskId: string,
    private readonly _field: string
  ) {
    super();
  }

  rhaiExpr(): string {
    return `tasks.${this._taskId}.output.${this._field}`;
  }
}

/**
 * Reference to a task's entire output object: tasks.<id>.output
 * Use when passing the whole output to another operator (e.g., board=).
 */
export class TaskOutputRef extends Ref {
  constructor(private readonly _taskId: string) {
    super();
  }

  rhaiExpr(): string {
    return `tasks.${this._taskId}.output`;
  }
}

/**
 * Proxy returned by Task.out — defers field lookup until property access.
 * task.out.field returns an OutRef for that specific field.
 * task.out itself can be used as a TaskOutputRef (whole output object).
 */
export type OutProxy = TaskOutputRef & {
  [field: string]: OutRef;
};

export function makeOutProxy(taskId: string): OutProxy {
  const base = new TaskOutputRef(taskId);
  return new Proxy(base as unknown as OutProxy, {
    get(target, prop: string | symbol) {
      if (typeof prop === "symbol") return Reflect.get(target, prop);
      // Pass through built-in methods/properties on TaskOutputRef
      if (prop in target) return Reflect.get(target, prop);
      // Any other property access is treated as an output field
      return new OutRef(taskId, prop);
    },
  });
}

/**
 * Reference to a workflow trigger payload field: triggers.<name>
 */
export class InputRef extends Ref {
  constructor(private readonly _name: string) {
    super();
  }

  rhaiExpr(): string {
    return `triggers.${this._name}`;
  }
}

/**
 * Proxy for wf.input.<name> — returns InputRef on property access.
 */
export type ScopeProxy = { [name: string]: InputRef | ContextRef };

export function makeInputProxy(): ScopeProxy {
  return new Proxy({} as ScopeProxy, {
    get(_target, prop: string | symbol) {
      if (typeof prop === "symbol") return undefined;
      return new InputRef(prop);
    },
  });
}

/**
 * Reference to a workflow context variable: context.<name>
 */
export class ContextRef extends Ref {
  constructor(private readonly _name: string) {
    super();
  }

  rhaiExpr(): string {
    return `context.${this._name}`;
  }
}

/**
 * Proxy for wf.context.<name> / wf.var.<name> — returns ContextRef on property access.
 */
export function makeContextProxy(): ScopeProxy {
  return new Proxy({} as ScopeProxy, {
    get(_target, prop: string | symbol) {
      if (typeof prop === "symbol") return undefined;
      return new ContextRef(prop as string);
    },
  });
}

/**
 * Reference to an environment variable: env("VAR")
 */
export class EnvRef extends Ref {
  constructor(private readonly _varName: string) {
    super();
  }

  rhaiExpr(): string {
    return `env("${this._varName}")`;
  }
}

/**
 * Reference to an injected ambient variable (declared via wf.expects).
 * Renders as the bare variable name (no prefix).
 */
export class AmbientRef extends Ref {
  constructor(private readonly _name: string) {
    super();
  }

  rhaiExpr(): string {
    return this._name;
  }
}

/**
 * Opaque passthrough — wraps a raw Rhai expression string as a Guard.
 */
export function expr(raw: string): Guard {
  return new Guard(raw);
}

/**
 * Guard helper — wraps a Guard, Ref, or string into a Guard.
 */
export function when(guard: Guard | Ref | string): Guard {
  if (guard instanceof Guard) return guard;
  if (guard instanceof Ref) return new Guard(guard.rhaiExpr());
  return new Guard(guard);
}

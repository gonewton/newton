/**
 * Edge / transition helpers.
 *
 * Transitions are added via Task.then() and Task.otherwise().
 * Priority is determined by declaration order:
 *   - First .then() call gets priority 0
 *   - Next gets priority 5, 10, 15, ...
 *   - .otherwise() always uses current priority (no guard), then advances
 */

import type { Guard } from "./refs.js";

export const PRIORITY_STEP = 5;

export interface EdgeDict {
  to: string;
  priority?: number;
  when?: { $expr: string };
  label?: string;
}

/**
 * Internal representation of a single transition edge before compilation.
 */
export class EdgeSpec {
  constructor(
    public readonly targetId: string,
    public readonly priority: number,
    public readonly guard: Guard | null = null,
    public readonly label: string | null = null
  ) {}

  toDict(): EdgeDict {
    const d: EdgeDict = { to: this.targetId };
    if (this.priority !== 100) {
      d.priority = this.priority;
    }
    if (this.guard !== null) {
      d.when = this.guard.toCondition();
    }
    if (this.label !== null) {
      d.label = this.label;
    }
    return d;
  }
}

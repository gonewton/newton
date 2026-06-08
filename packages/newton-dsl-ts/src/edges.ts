/**
 * Edge / transition helpers.
 *
 * Transitions are added via Task.then() and Task.otherwise().
 * Priority is determined by declaration order:
 *   - First .then() call gets priority 0
 *   - Next gets priority 5, 10, 15, ...
 *   - .otherwise() always gets PRIORITY_OTHERWISE (a large sentinel), so it
 *     sorts last regardless of call order.
 */

import type { Guard } from "./refs.js";

export const PRIORITY_STEP = 5;
/** Sentinel used by Task.otherwise() — guaranteed to sort after any .then() edge. */
export const PRIORITY_OTHERWISE = 2_147_483_647; // i32::MAX

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
    const d: EdgeDict = { to: this.targetId, priority: this.priority };
    if (this.guard !== null) {
      d.when = this.guard.toCondition();
    }
    if (this.label !== null) {
      d.label = this.label;
    }
    return d;
  }
}

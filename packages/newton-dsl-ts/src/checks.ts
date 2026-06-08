/**
 * Compiler checks — run before YAML serialization.
 *
 * Checks performed:
 * 1. Dangling references: every edge target must be a defined task
 * 2. Reachability: every task must be reachable from entry_task (warning for unreachable)
 * 3. Bounded cycles: every cycle must have at least one task with maxIterations set
 */

import type { Task } from "./task.js";

export class CompilerError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CompilerError";
  }
}

export interface CompilerWarning {
  message: string;
}

export function checkAll(
  tasks: Map<string, Task>,
  entryTaskId: string
): CompilerWarning[] {
  const warnings: CompilerWarning[] = [];

  // 1. Dangling references
  checkDangling(tasks);

  // 2. Reachability
  const unreachable = findUnreachable(tasks, entryTaskId);
  for (const taskId of unreachable) {
    warnings.push({
      message: `task '${taskId}' is unreachable from entry_task '${entryTaskId}'`,
    });
  }

  // 3. Bounded cycles
  checkBoundedCycles(tasks);

  return warnings;
}

function checkDangling(tasks: Map<string, Task>): void {
  const known = new Set(tasks.keys());
  for (const task of tasks.values()) {
    for (const edge of task._edges) {
      if (!known.has(edge.targetId)) {
        throw new CompilerError(
          `task '${task.taskId}' has transition to undefined task '${edge.targetId}'`
        );
      }
    }
  }
}

function findUnreachable(
  tasks: Map<string, Task>,
  entryTaskId: string
): string[] {
  if (!tasks.has(entryTaskId)) {
    throw new CompilerError(
      `entry_task '${entryTaskId}' is not defined in the workflow`
    );
  }

  const visited = new Set<string>();
  const queue = [entryTaskId];
  while (queue.length > 0) {
    const current = queue.shift()!;
    if (visited.has(current)) continue;
    visited.add(current);
    const task = tasks.get(current);
    if (!task) continue;
    for (const edge of task._edges) {
      if (!visited.has(edge.targetId)) {
        queue.push(edge.targetId);
      }
    }
  }

  const allIds = [...tasks.keys()];
  return allIds.filter((id) => !visited.has(id)).sort();
}

function checkBoundedCycles(tasks: Map<string, Task>): void {
  // Build adjacency list
  const graph = new Map<string, string[]>();
  for (const [tid, task] of tasks) {
    graph.set(tid, task._edges.map((e) => e.targetId));
  }

  // Tarjan's SCC algorithm
  const index = new Map<string, number>();
  const lowlink = new Map<string, number>();
  const onStack = new Set<string>();
  const stack: string[] = [];
  const sccs: string[][] = [];
  let indexCounter = 0;

  function strongconnect(v: string): void {
    index.set(v, indexCounter);
    lowlink.set(v, indexCounter);
    indexCounter++;
    stack.push(v);
    onStack.add(v);

    for (const w of (graph.get(v) ?? [])) {
      if (!index.has(w)) {
        strongconnect(w);
        lowlink.set(v, Math.min(lowlink.get(v)!, lowlink.get(w)!));
      } else if (onStack.has(w)) {
        lowlink.set(v, Math.min(lowlink.get(v)!, index.get(w)!));
      }
    }

    if (lowlink.get(v) === index.get(v)) {
      const scc: string[] = [];
      while (true) {
        const w = stack.pop()!;
        onStack.delete(w);
        scc.push(w);
        if (w === v) break;
      }
      sccs.push(scc);
    }
  }

  for (const v of graph.keys()) {
    if (!index.has(v)) {
      strongconnect(v);
    }
  }

  // Check each SCC: if it has >1 node OR has a self-loop, it's a real cycle
  for (const scc of sccs) {
    const sccSet = new Set(scc);
    const isCycle =
      scc.length > 1 ||
      (scc.length === 1 && (graph.get(scc[0]) ?? []).includes(scc[0]));
    if (!isCycle) continue;

    // Check that at least one task in the cycle has maxIterations set
    const hasCap = scc.some((tid) => {
      const t = tasks.get(tid);
      return t != null && t._maxIterations != null;
    });
    if (!hasCap) {
      throw new CompilerError(
        `unbounded cycle detected involving tasks: ${[...scc].sort().join(", ")}. ` +
          `Add repeatAtMost(N) to at least one task in the cycle.`
      );
    }
  }
}

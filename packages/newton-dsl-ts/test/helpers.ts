/**
 * Shared test helpers for round-trip conformance tests.
 */
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import * as Y from "yaml";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Resolve conformance root relative to this file: test/ → newton-dsl-ts/ → packages/ → repo root
const CONFORMANCE_ROOT = path.resolve(
  __dirname,
  "..",  // test/ → newton-dsl-ts/
  "..",  // newton-dsl-ts/ → packages/
  "workflow-schema",
  "conformance"
);

export function loadFixture(name: string): Record<string, unknown> {
  const fixturePath = path.join(CONFORMANCE_ROOT, "cases", name, "expected.yaml");
  if (!fs.existsSync(fixturePath)) {
    throw new Error(`Fixture not found: ${fixturePath}`);
  }
  return Y.parse(fs.readFileSync(fixturePath, "utf-8")) as Record<string, unknown>;
}

/**
 * Deep-sort object keys for semantic comparison.
 */
export function deepSort(obj: unknown): unknown {
  if (Array.isArray(obj)) return obj.map(deepSort);
  if (obj !== null && typeof obj === "object") {
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(obj as Record<string, unknown>).sort()) {
      sorted[key] = deepSort((obj as Record<string, unknown>)[key]);
    }
    return sorted;
  }
  return obj;
}

/**
 * Normalize a task dict for semantic comparison:
 * - Drop priority from transitions (declaration order determines semantics)
 * - Drop empty transitions array
 * - Remove capture_stdout: false and capture_stderr: false (treat as absent)
 * - Sort params keys
 * - Remove null/undefined values
 */
function normalizeTask(t: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = { ...t };

  // Normalize transitions: strip priority, keep order + conditions
  const transitions = (t.transitions as Record<string, unknown>[] | undefined) ?? [];
  const normalizedTransitions = transitions.map((tr: Record<string, unknown>) => {
    const nt: Record<string, unknown> = { to: tr.to };
    if (tr.when != null) nt.when = tr.when;
    if (tr.label != null) nt.label = tr.label;
    return nt;
  });
  if (normalizedTransitions.length > 0) {
    result.transitions = normalizedTransitions;
  } else {
    delete result.transitions;
  }

  // Normalize params
  if (result.params != null && typeof result.params === "object") {
    let params = { ...(result.params as Record<string, unknown>) };
    // Remove capture_stdout: false (treat as absent)
    if (params.capture_stdout === false) delete params.capture_stdout;
    if (params.capture_stderr === false) delete params.capture_stderr;
    result.params = deepSort(params) as Record<string, unknown>;
  }

  // Remove null/undefined values
  for (const [k, v] of Object.entries(result)) {
    if (v == null) delete result[k];
  }

  // Sort all keys recursively
  return deepSort(result) as Record<string, unknown>;
}

/**
 * Compare two task lists semantically (by id, order-insensitive for individual fields).
 * Returns { equal: boolean, diff: string }.
 */
export function tasksSemanticEqual(
  actual: unknown[],
  expected: unknown[]
): { equal: boolean; diff: string } {
  const actualById = new Map<string, Record<string, unknown>>();
  for (const t of actual) {
    const task = t as Record<string, unknown>;
    actualById.set(task.id as string, normalizeTask(task));
  }

  const expectedById = new Map<string, Record<string, unknown>>();
  for (const t of expected) {
    const task = t as Record<string, unknown>;
    expectedById.set(task.id as string, normalizeTask(task));
  }

  const diffs: string[] = [];

  // Check for missing tasks
  for (const id of expectedById.keys()) {
    if (!actualById.has(id)) {
      diffs.push(`Missing task '${id}' in actual output`);
    }
  }

  // Check for extra tasks
  for (const id of actualById.keys()) {
    if (!expectedById.has(id)) {
      diffs.push(`Extra task '${id}' in actual output not in expected`);
    }
  }

  // Compare common tasks
  for (const id of expectedById.keys()) {
    const a = actualById.get(id);
    const e = expectedById.get(id);
    if (a == null || e == null) continue;
    const aStr = JSON.stringify(a, null, 2);
    const eStr = JSON.stringify(e, null, 2);
    if (aStr !== eStr) {
      diffs.push(`Task '${id}' differs:\n  ACTUAL:\n${aStr}\n  EXPECTED:\n${eStr}`);
    }
  }

  if (diffs.length === 0) return { equal: true, diff: "" };
  return { equal: false, diff: diffs.join("\n\n") };
}

export function normalizeSettings(settings: unknown): Record<string, unknown> {
  return deepSort(settings) as Record<string, unknown>;
}

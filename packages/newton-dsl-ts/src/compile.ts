/**
 * Compiler — builds the WorkflowDocument dict and serializes to YAML.
 *
 * Flow:
 *   1. Run checks.ts (dangling refs, reachability, bounded cycles)
 *   2. Build WorkflowDocument dict
 *   3. Serialize to YAML via `yaml` package
 */

import * as Y from "yaml";
import { checkAll } from "./checks.js";
import type { Workflow } from "./workflow.js";

function removeNull(obj: unknown): unknown {
  if (obj === null || obj === undefined) return undefined;
  if (Array.isArray(obj)) {
    return obj
      .map(removeNull)
      .filter((v) => v !== undefined);
  }
  if (typeof obj === "object") {
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
      const cleaned = removeNull(v);
      if (cleaned !== undefined) {
        result[k] = cleaned;
      }
    }
    return result;
  }
  return obj;
}

/**
 * Compile a Workflow object into a YAML string.
 * Calls console.warn for non-fatal warnings; throws CompilerError on fatal issues.
 */
export function compileWorkflow(wf: Workflow): string {
  // Run checks
  const warnings = checkAll(wf._tasks, wf._entryTask!);
  for (const w of warnings) {
    console.warn(`CompilerWarning: ${w.message}`);
  }

  const doc = buildDocument(wf);
  const cleaned = removeNull(doc);

  return Y.stringify(cleaned, {
    lineWidth: 120,
    defaultKeyType: "PLAIN",
    defaultStringType: "PLAIN",
    nullStr: "",
    blockSeq: true,
    blockQuote: true,
  });
}

function buildDocument(wf: Workflow): Record<string, unknown> {
  return {
    version: "2.0",
    mode: "workflow_graph",
    metadata: wf._metadata,
    triggers: wf._triggers,
    workflow: {
      settings: buildSettings(wf),
      context: wf._context,
      tasks: wf._taskList.map((t) => t.toDict()),
    },
  };
}

function buildSettings(wf: Workflow): Record<string, unknown> {
  const s: Record<string, unknown> = {};
  if (wf._entryTask != null) s.entry_task = wf._entryTask;
  if (wf._maxTimeSeconds != null) s.max_time_seconds = wf._maxTimeSeconds;
  if (wf._parallelLimit != null) s.parallel_limit = wf._parallelLimit;
  if (wf._continueOnError != null) s.continue_on_error = wf._continueOnError;
  if (wf._maxTaskIterations != null) s.max_task_iterations = wf._maxTaskIterations;
  if (wf._maxWorkflowIterations != null) s.max_workflow_iterations = wf._maxWorkflowIterations;
  if (wf._defaultEngine != null) s.default_engine = wf._defaultEngine;
  if (wf._commandOperatorSettings != null) s.command_operator = wf._commandOperatorSettings;
  if (wf._artifactStorage != null) s.artifact_storage = wf._artifactStorage;
  return s;
}

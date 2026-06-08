/**
 * @newton/dsl — TypeScript authoring surface for Newton workflow graphs.
 *
 * @example
 * ```ts
 * import { Workflow, command, agent, gh, expr } from "@newton/dsl";
 *
 * const wf = new Workflow("my-workflow", { defaultEngine: "codex" });
 * wf.inputs({ prompt: "" });
 *
 * const t1 = wf.task("run_check", command({ cmd: "./check.sh", shell: true, captureStdout: true }));
 * const t2 = wf.finish("done");
 * t1.then(t2);
 *
 * console.log(wf.toYaml());
 * ```
 */

// Layer 2 — ergonomic builder
export { Workflow } from "./workflow.js";
export { Task } from "./task.js";
export { EdgeSpec, PRIORITY_STEP } from "./edges.js";
export {
  command,
  agent,
  humanApproval,
  subWorkflow,
  gh,
  OperatorCall,
} from "./operators.js";
export {
  Ref,
  Guard,
  OutRef,
  TaskOutputRef,
  InputRef,
  ContextRef,
  EnvRef,
  AmbientRef,
  makeOutProxy,
  makeInputProxy,
  makeContextProxy,
  expr,
  when,
} from "./refs.js";
export { CompilerError } from "./checks.js";

// Layer 1 — generated types
export type {
  WorkflowDocument,
  WorkflowTask,
  WorkflowSettings,
  WorkflowDefinition,
  WorkflowMetadata,
  WorkflowTrigger,
  Transition,
  Condition,
  RetryPolicy,
  TerminalKind,
  ArtifactStorageSettings,
} from "./generated/ir.js";

// Runtime validation
export { validateWorkflowDocument } from "./generated/validate.js";

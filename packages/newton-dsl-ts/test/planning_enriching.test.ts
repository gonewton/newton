/**
 * Round-trip test for planning_enriching.yaml.
 *
 * Authors the workflow in TypeScript using @newton/dsl, compiles to YAML,
 * and asserts semantic equality with the conformance fixture.
 *
 * Also verifies that:
 * - The compiler warns (console.warn) about task-1 (unreachable)
 * - A workflow with an unbounded cycle throws CompilerError
 * - A workflow with a dangling edge throws CompilerError
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import * as Y from "yaml";
import { Workflow, agent, command, expr, AmbientRef, CompilerError, Task } from "../src/index.js";
import { OperatorCall } from "../src/operators.js";
import { EdgeSpec } from "../src/edges.js";
import { loadFixture, tasksSemanticEqual, normalizeSettings } from "./helpers.js";

function buildPlanningEnriching(): Workflow {
  const wf = new Workflow("planning-enriching", {
    description:
      "Pure spec enrichment (no GitHub). Invoked via WorkflowOperator from planner.yaml, " +
      "or standalone: newton run planning_enriching.yaml --arg prompt=... --arg output_path=... (--set engines).",
    defaultEngine: "codex",
    parallelLimit: 1,
    maxTimeSeconds: 999999999,
    continueOnError: false,
    maxTaskIterations: 3,
    maxWorkflowIterations: 15,
    allowShell: true,
  });

  wf.inputs({ prompt: "", output_path: "" });
  wf.expects("develop_primary_engine", "develop_primary_model");

  const engine = new AmbientRef("develop_primary_engine");
  const model = new AmbientRef("develop_primary_model");

  // ----------------------------------------------------------------
  // enrich_spec
  // ----------------------------------------------------------------
  const enrichPrompt = expr(
    'context.preamble + "\\n\\nOutput format:\\n' +
    '- Write the full enriched markdown to the file " + triggers.output_path + " in the workspace (create directory if needed).\\n' +
    '- Write the list of aspect names that need user input (one per line) to .newton/plan/gaps.txt. If none need input, write a single line \\"none\\". ' +
    'Use only these aspect names when listing gaps: problem_statement, goals, current_behavior, design, schema_api, error_codes, acceptance_criteria, ' +
    'functionality_comparison, benefit, stages, breaking_changes, modified_files_table, dependencies, folder_structure, design_decisions, out_of_scope, references.\\n\\n' +
    '---\\nUser spec (path or content):\\n\\n" + triggers.prompt'
  );

  const enrichSpec = wf.task("enrich_spec", agent({ engine, model, prompt: enrichPrompt }));
  enrichSpec.repeatAtMost(2);

  // ----------------------------------------------------------------
  // check_gaps
  // ----------------------------------------------------------------
  const checkGapsCmd =
    "set -e\n" +
    'if [ ! -f "$OUT" ]; then\n' +
    '  echo "check_gaps: enriched spec missing at ${OUT} (enrich agent must create this path)" >&2\n' +
    "  exit 1\n" +
    "fi\n" +
    'if grep -q "NEED_USER_INPUT" "$OUT"; then\n' +
    '  printf "has_gaps"\n' +
    "else\n" +
    '  printf "no_gaps"\n' +
    "fi\n";

  const checkGaps = wf.task(
    "check_gaps",
    command({
      cmd: checkGapsCmd,
      shell: true,
      env: { OUT: wf.input.output_path },
      captureStdout: true,
    })
  );

  // ----------------------------------------------------------------
  // cat_gaps
  // ----------------------------------------------------------------
  const catGaps = wf.task(
    "cat_gaps",
    command({
      cmd: "cat .newton/plan/gaps.txt 2>/dev/null || echo 'none'",
      shell: true,
      captureStdout: true,
    })
  );

  // ----------------------------------------------------------------
  // clarify_spec
  // ----------------------------------------------------------------
  const clarifyPrompt = expr(
    '"You are running the newton-clarify-question protocol for a spec that still has unresolved gaps.\\n\\n' +
    'Spec path: " + triggers.output_path + "\\nGaps file: .newton/plan/gaps.txt\\n\\n' +
    'Steps:\\n1. Read the spec and collect every NEED_USER_INPUT tag with surrounding context.\\n' +
    '2. For each gap, ground at least two concrete alternatives in the actual codebase, lockfiles, and dependencies. ' +
    'Prefer evidence over guesses; list unverifiable claims in evidence_gaps.\\n' +
    '3. Pick a recommended alternative (primary_alternative_id must match one of alternatives[].id).\\n\\n' +
    'Output a single ```json fenced code block containing one object that validates against this schema (schema_version must be 1):\\n' +
    '{\\n  \\"schema_version\\": 1,\\n  \\"spec_paths\\": [\\"<" + triggers.output_path + ">\\"],\\n' +
    '  \\"need_user_input\\": [{\\"tag\\": string, \\"excerpt\\": string, \\"location_hint\\": string}],\\n' +
    '  \\"context_notes\\": [string],\\n  \\"evidence_gaps\\": [string],\\n  \\"problem_statement\\": string,\\n' +
    '  \\"alternatives\\": [{\\"id\\": \\"<slug: ^[a-z][a-z0-9_-]*$>\\", \\"title\\": string, \\"pros\\": [string], \\"cons\\": [string]}],\\n' +
    '  \\"recommendation\\": {\\"summary\\": string, \\"rationale\\": string, \\"primary_alternative_id\\": string, \\"confidence\\": \\"low\\"|\\"medium\\"|\\"high\\"}\\n' +
    '}\\n\\nRules:\\n- alternatives must have at least 2 entries\\n' +
    '- alternatives[].id must be lowercase slugs (^[a-z][a-z0-9_-]*$)\\n' +
    '- recommendation.primary_alternative_id must equal one of the alternatives[].id values\\n' +
    '- Do NOT include defer or abort alternatives; the workflow adds those\\n\\n' +
    'After producing the JSON block, write the JSON object (not the markdown mirror) to .newton/plan/clarify.json."'
  );

  const clarifySpec = wf.task("clarify_spec", agent({ engine, model, prompt: clarifyPrompt }));
  clarifySpec.repeatAtMost(2);

  // ----------------------------------------------------------------
  // validate_clarify
  // ----------------------------------------------------------------
  const validateCmd =
    "set -e\n" +
    'CLARIFY=".newton/plan/clarify.json"\n' +
    'if [ ! -f "$CLARIFY" ]; then\n' +
    '  echo "ERROR: clarify.json not written by clarify_spec agent" >&2\n' +
    "  exit 1\n" +
    "fi\n" +
    "jq -e '\n" +
    "  .schema_version == 1\n" +
    "  and (.spec_paths | length > 0)\n" +
    "  and (.need_user_input | length > 0)\n" +
    "  and (.problem_statement | length > 0)\n" +
    "  and (.alternatives | length >= 2)\n" +
    "  and (.recommendation.primary_alternative_id != null)\n" +
    "  and (\n" +
    "    .recommendation.primary_alternative_id as $rid\n" +
    "    | [.alternatives[].id] | contains([$rid])\n" +
    "  )\n" +
    "' \"$CLARIFY\" > /dev/null\n";

  const validateClarify = wf.task(
    "validate_clarify",
    command({ cmd: validateCmd, shell: true, captureStdout: false })
  );

  // ----------------------------------------------------------------
  // present_decision
  // ----------------------------------------------------------------
  const presentCmd =
    "set -e\n" +
    'CLARIFY=".newton/plan/clarify.json"\n' +
    "PAYLOAD=$(jq -c '{\n" +
    '  decision_id: ("planning-enriching-" + (now | floor | tostring)),\n' +
    "  summary: .problem_statement,\n" +
    "  context_markdown: (\n" +
    '    "**Decision required for:** " + .problem_statement\n' +
    '    + "\\n\\n**Open gaps:**\\n"\n' +
    '    + (.need_user_input | map("- **\\(.tag)**: \\(.excerpt) _(\\(.location_hint))_") | join("\\n"))\n' +
    "  ),\n" +
    "  options: (\n" +
    "    (.alternatives | map({\n" +
    "      id: .id,\n" +
    "      label: .title,\n" +
    "      detail_markdown: (\n" +
    '        "**Pros:** " + (.pros | join(" · "))\n' +
    '        + "\\n\\n**Cons:** " + (.cons | join(" · "))\n' +
    "      )\n" +
    "    }))\n" +
    "    + [\n" +
    '      {"id": "defer", "label": "Defer — keep placeholders, revisit later"},\n' +
    '      {"id": "abort", "label": "Abort — stop enrichment without merging"}\n' +
    "    ]\n" +
    "  ),\n" +
    "  recommendation: {\n" +
    "    option_id: .recommendation.primary_alternative_id,\n" +
    "    rationale_markdown: (\n" +
    "      .recommendation.rationale\n" +
    '      + ((.recommendation.confidence // "") | if . != "" then " *(confidence: " + . + ")*" else "" end)\n' +
    "    )\n" +
    "  }\n" +
    "}' \"$CLARIFY\")\n" +
    'CHOICE=$(ailoop ask --payload "$PAYLOAD" --json | jq -r \'.response\')\n' +
    'printf "%s" "$CHOICE"\n';

  const presentDecision = wf.task(
    "present_decision",
    command({ cmd: presentCmd, shell: true, captureStdout: true })
  );

  // ----------------------------------------------------------------
  // merge_spec
  // ----------------------------------------------------------------
  const mergePrompt = expr(
    '"Read the enriched spec at " + triggers.output_path + " and the clarifier output at .newton/plan/clarify.json.\\n\\n' +
    'The human selected option id: \\"" + tasks.present_decision.output.stdout + "\\"\\n\\n' +
    'Find that alternative in clarify.json (alternatives[].id == \\"" + tasks.present_decision.output.stdout + "\\"). ' +
    'If the id is not found (e.g. fallback), use the recommendation instead.\\n\\n' +
    'For every NEED_USER_INPUT placeholder in the spec:\\n' +
    '- Replace it with a concrete resolution grounded in the chosen alternative (title, pros, cons from clarify.json).\\n' +
    '- If a placeholder cannot be fully resolved by the chosen alternative, document it as an explicit design decision or open question — never leave bare NEED_USER_INPUT tags.\\n\\n' +
    'Write the resolved spec back to " + triggers.output_path + " (overwrite in place). Leave sections without placeholders unchanged."'
  );

  const mergeSpec = wf.task("merge_spec", agent({ engine, model, prompt: mergePrompt }));
  mergeSpec.repeatAtMost(1);

  // ----------------------------------------------------------------
  // finalize
  // ----------------------------------------------------------------
  const finalize = wf.task(
    "finalize",
    command({
      cmd: "echo Enriched spec written to $OUT",
      shell: true,
      env: { OUT: wf.input.output_path },
      captureStdout: false,
    })
  );
  finalize._terminal = "success";

  // ----------------------------------------------------------------
  // task-1 (dead task — unreachable, compiler should warn)
  // ----------------------------------------------------------------
  const task1 = new Task("task-1", new OperatorCall("AgentOperator", {}));
  wf._tasks.set("task-1", task1);
  wf._taskList.push(task1);

  // ----------------------------------------------------------------
  // Wire transitions
  // ----------------------------------------------------------------
  enrichSpec.then(checkGaps);

  checkGaps
    .then(catGaps, { when: expr('tasks.check_gaps.output.stdout == "has_gaps"') })
    .then(finalize, { when: expr('tasks.check_gaps.output.stdout == "no_gaps"') });

  catGaps.then(clarifySpec);
  clarifySpec.then(validateClarify);
  validateClarify.then(presentDecision);

  presentDecision
    .then(finalize, { when: expr('tasks.present_decision.output.stdout == "defer"') })
    .then(finalize, { when: expr('tasks.present_decision.output.stdout == "abort"') })
    .then(mergeSpec);

  mergeSpec.then(finalize);

  return wf;
}

describe("planning_enriching round-trip", () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warnSpy.mockRestore();
  });

  it("compiles without throwing CompilerError", () => {
    const wf = buildPlanningEnriching();
    const yaml = wf.toYaml();
    expect(yaml).toBeTruthy();
  });

  it("compiler warns about unreachable task-1", () => {
    const wf = buildPlanningEnriching();
    wf.toYaml();

    const warnCalls = warnSpy.mock.calls.map((c) => String(c[0]));
    const hasUnreachableWarn = warnCalls.some(
      (msg) => msg.includes("task-1") && msg.toLowerCase().includes("unreachable")
    );
    expect(hasUnreachableWarn).toBe(true);
  });

  it("is semantically equal to the conformance fixture", () => {
    const wf = buildPlanningEnriching();
    const yamlStr = wf.toYaml();
    const compiled = Y.parse(yamlStr) as Record<string, unknown>;
    const expected = loadFixture("planning_enriching");

    expect(compiled.version).toBe(expected.version);
    expect(compiled.mode).toBe(expected.mode);

    const compiledMeta = compiled.metadata as Record<string, unknown>;
    const expectedMeta = expected.metadata as Record<string, unknown>;
    expect(compiledMeta?.name).toBe(expectedMeta?.name);

    const cWf = compiled.workflow as Record<string, unknown>;
    const eWf = expected.workflow as Record<string, unknown>;
    const cSettings = normalizeSettings(cWf.settings);
    const eSettings = normalizeSettings(eWf.settings);

    for (const key of [
      "entry_task",
      "parallel_limit",
      "continue_on_error",
      "max_task_iterations",
      "max_workflow_iterations",
      "default_engine",
    ]) {
      expect(cSettings[key]).toEqual(eSettings[key]);
    }

    const { equal, diff } = tasksSemanticEqual(
      cWf.tasks as unknown[],
      eWf.tasks as unknown[]
    );
    expect(equal, `Task semantic mismatch:\n${diff}`).toBe(true);
  });

  it("validates against the JSON Schema", async () => {
    const wf = buildPlanningEnriching();
    const yamlStr = wf.toYaml();
    const compiled = Y.parse(yamlStr);

    const { validateWorkflowDocument } = await import("../src/generated/validate.js");
    const result = validateWorkflowDocument(compiled);
    expect(result.valid, `Schema validation failed:\n${result.errors.join("\n")}`).toBe(true);
  });

  it("rejects an unbounded cycle", () => {
    const wf = new Workflow("test-cycle");
    const a = wf.task("a", command({ cmd: "echo a" }));
    const b = wf.task("b", command({ cmd: "echo b" }));
    a.then(b);
    b.then(a); // back-edge with no cap

    expect(() => wf.toYaml()).toThrow(CompilerError);
    expect(() => wf.toYaml()).toThrow(/unbounded cycle/);
  });

  it("rejects a dangling edge to an undefined task", () => {
    const wf = new Workflow("test-dangling");
    const a = wf.task("a", command({ cmd: "echo a" }));
    // Manually inject a dangling edge
    a._edges.push(new EdgeSpec("nonexistent", 0));

    expect(() => wf.toYaml()).toThrow(CompilerError);
    expect(() => wf.toYaml()).toThrow(/undefined task/);
  });
});

/**
 * Round-trip test for planner.yaml.
 *
 * Authors the workflow in TypeScript using @newton/dsl, compiles to YAML,
 * and asserts semantic equality with the conformance fixture.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import * as Y from "yaml";
import {
  Workflow,
  command,
  gh,
  subWorkflow,
  expr,
  AmbientRef,
} from "../src/index.js";
import { loadFixture, tasksSemanticEqual, normalizeSettings } from "./helpers.js";

const PREAMBLE =
  "You are enriching a product/spec document so that requirements are clearly and explicitly defined " +
  "and implementation-ready. The user provided a spec (path or content below).\n\n" +
  "CODEBASE GROUNDING (do this first):\n" +
  "Before writing the enriched document, you MUST explore the target codebase (read key files, " +
  "list directories) to ground every section in the actual project structure. File paths in the " +
  '"Modified files" table MUST correspond to real files. If the spec references entities ' +
  "(commands, endpoints, modules), enumerate them from the source, not from the user's prose.\n\n" +
  "You MUST produce an enriched markdown document that MANDATORILY covers these aspects in this " +
  "order (use the exact section headers):\n\n" +
  "TECHNICAL CONTEXT (sections 1-7):\n" +
  '1. **Problem statement** - What is broken or missing today. One paragraph grounded in actual ' +
  'codebase state. MUST include an explicit "out of scope" sentence for related work not in this spec.\n' +
  "2. **Goals** - Numbered list of concrete outcomes. Each goal MUST be verifiable.\n" +
  "3. **Current behavior** - Table: Component | Role | Mutable at runtime? Plus 1-2 sentences on " +
  "current build and resume flow. Grounded in code inspection.\n" +
  '4. **Design** - The core "how". Subsections for each major mechanism (data structures, ' +
  "concurrency model, API surface, integration points). MUST include: storage types and data structure " +
  "choices with concrete types, concurrency/locking strategy, invariants, identity/naming conventions, " +
  "collision and error conditions, tick/lifecycle visibility rules. Include YAML or config examples " +
  "where applicable.\n" +
  "5. **Schema and API** - Exact struct/type definitions and method signatures that implementers will " +
  "create or modify. Use the project's language (Rust, Python, etc.) for code blocks.\n" +
  "6. **Error codes** - Table: Code | Category | Trigger. One row per error the implementation " +
  "introduces.\n" +
  "7. **Acceptance criteria** - Numbered, testable conditions. Each criterion MUST be a single " +
  "verifiable statement (not a vague goal). Produce at least one criterion per goal, one per error " +
  "code, and one for backward-compatibility regression.\n\n" +
  "PLANNING (sections 8-14):\n" +
  "8. **Functionality comparison (before vs after)** - What exists today vs what will exist after " +
  "implementation.\n" +
  "9. **Benefit of implementing this functionality** - Why implement this; value and impact.\n" +
  "10. **Stages** - Break the work into ordered stages. Each stage MUST define: (a) a concrete " +
  'deliverable or output, (b) the explicit scope (enumerate items, NEVER use "etc." or ellipsis), ' +
  "and (c) any blocking dependency on a prior stage. If a stage produces a document or artifact " +
  "that later stages consume, describe its expected content or structure.\n" +
  "11. **Breaking changes** - API, config, or behavioral changes that affect existing users or callers.\n" +
  "12. **Modified files and summary** - A table: | File/path | Summary of modifications |. Every " +
  "path MUST be verified against the actual codebase. If existing tests or configs already cover " +
  "the area, reference them.\n" +
  "13. **Dependencies** - External or internal dependencies (libraries, services, teams). For each " +
  'dependency on a person or team, state whether it is blocking or non-blocking and which stage it ' +
  'gates. If a dependency is marked "recommended" or "optional", state what happens if it is ' +
  "skipped.\n" +
  "14. **Proposed folder structure** - If this is a new project, write complete folder structure; " +
  "otherwise only modified files and paths.\n\n" +
  "VERIFICATION AND REFERENCE (sections 15-17):\n" +
  "15. **Design decisions** - Table: # | Question | Decision | Rationale. Captures key trade-offs " +
  "so implementers do not re-debate them.\n" +
  "16. **Out of scope (explicit)** - Bulleted list of things explicitly excluded. Prevents scope creep.\n" +
  "17. **References** - Pointers to real files, functions, and structs in the codebase that the " +
  "implementer should start from.\n\n" +
  "Requirements quality (apply throughout the spec):\n" +
  "- State requirements in normative language where applicable: MUST (mandatory), SHOULD " +
  "(recommended), MAY (optional). Avoid vague wording; make obligations and options explicit.\n" +
  "- For each major capability or command, make applicability explicit (e.g. which commands get " +
  'which options, or "all read-only commands" with a defined list).\n' +
  '- NEVER use "etc.", "and so on", or ellipsis to scope work items. If a stage or requirement ' +
  "applies to a subset of entities, enumerate them explicitly. If the full list is unknown, flag " +
  "it with [NEED_USER_INPUT].\n" +
  '- Include test expectations where relevant (e.g. "Format selection MUST be covered by tests ' +
  'for each updated command").\n' +
  "- Ensure success criteria are measurable or verifiable. When a criterion uses a comparative " +
  'metric (e.g. "2x longer", "50% faster"), it MUST include or reference a concrete baseline ' +
  "value so the criterion can be checked.\n" +
  "- If the spec defines options or formats (e.g. output formats, flags), specify per-entity " +
  "applicability or a clear rule so implementers know exactly where each option applies.\n\n" +
  "For each of the 17 aspects: if you can fill it from the spec, codebase inspection, or " +
  "well-grounded inference, write the section. If you are inventing names, paths, or details that " +
  "cannot be verified from the spec or codebase, write exactly \"[NEED_USER_INPUT: <aspect_name>]\" " +
  "in that section and add the aspect_name to the gaps list. Valid aspect_name values: " +
  "problem_statement, goals, current_behavior, design, schema_api, error_codes, acceptance_criteria, " +
  "functionality_comparison, benefit, stages, breaking_changes, modified_files_table, dependencies, " +
  "folder_structure, design_decisions, out_of_scope, references.\n\n" +
  "When presenting multiple options to the user, present a recommended one and the rationale.";

function buildPlanner(): Workflow {
  const wf = new Workflow("planner-workflow", {
    description:
      "Orchestrator: resolve board, slug paths; copy planning_enriching into workspace " +
      ".newton/workflows; WorkflowOperator loads it via triggers.workspace (nested sandbox); " +
      "inject frontmatter; publish issue body; move item to Backlog.",
    parallelLimit: 1,
    maxTimeSeconds: 999999999,
    continueOnError: false,
    maxTaskIterations: 3,
    maxWorkflowIterations: 15,
    allowShell: true,
    defaultEngine: "codex",
  });

  wf.inputs({
    raw_spec_path: "",
    board_item_id: "",
    board_content_id: "",
    board_issue_number: "",
    board_item_title: "",
  });

  wf.setContext({ preamble: PREAMBLE });
  wf.expects("develop_primary_engine", "develop_primary_model");

  // ----------------------------------------------------------------
  // resolve_board_ids
  // ----------------------------------------------------------------
  const resolveBoardIds = wf.task(
    "resolve_board_ids",
    gh.projectResolveBoard({
      owner: expr('env("GH_PROJECT_OWNER")'),
      projectNumber: expr('env("GH_PROJECT_NUMBER")'),
      requiredOptionNames: ["Backlog"],
    })
  );

  // ----------------------------------------------------------------
  // compute_paths
  // ----------------------------------------------------------------
  const computePaths = wf.task(
    "compute_paths",
    command({
      cmd:
        "set -eo pipefail\n" +
        "slug=$(printf '%s' \"${TITLE:-}\" | iconv -f utf-8 -t ascii//TRANSLIT 2>/dev/null || printf '%s' \"${TITLE:-}\")\n" +
        "slug=$(printf '%s' \"$slug\" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')\n" +
        "if [ -z \"$slug\" ]; then slug=\"spec\"; fi\n" +
        "slug=$(printf '%s' \"$slug\" | cut -c1-60 | sed -E 's/-+$//')\n" +
        "mkdir -p .newton/plan tmp\n" +
        "OUT=\"tmp/${ISSUE}-${slug}.md\"\n" +
        "BRANCH=\"feature/${ISSUE}-${slug}\"\n" +
        "printf '%s\\n' \"$OUT\" > .newton/plan/.planner-output-path\n" +
        "printf '%s\\n' \"$BRANCH\" > .newton/plan/.planner-branch\n" +
        "echo \"output_path=$OUT branch=$BRANCH\"\n",
      shell: true,
      captureStdout: true,
      env: {
        ISSUE: wf.input.board_issue_number,
        TITLE: wf.input.board_item_title,
      },
    })
  );

  // ----------------------------------------------------------------
  // read_output_path
  // ----------------------------------------------------------------
  const readOutputPath = wf.task(
    "read_output_path",
    command({
      cmd: "set -eo pipefail\nprintf '%s' \"$(cat .newton/plan/.planner-output-path)\"\n",
      shell: true,
      captureStdout: true,
    })
  );

  // ----------------------------------------------------------------
  // invoke_enricher (WorkflowOperator / subWorkflow)
  // ----------------------------------------------------------------
  const invokeEnricher = wf.task(
    "invoke_enricher",
    subWorkflow({
      workflowPath: expr('triggers.workspace + "/.newton/workflows/planning_enriching.yaml"'),
      triggers: {
        prompt: wf.input.raw_spec_path,
        output_path: readOutputPath.out.stdout,
      },
      context: {
        preamble: wf.context.preamble,
        develop_primary_engine: new AmbientRef("develop_primary_engine"),
        develop_primary_model: new AmbientRef("develop_primary_model"),
      },
    })
  );

  // ----------------------------------------------------------------
  // inject_frontmatter
  // ----------------------------------------------------------------
  const injectFrontmatter = wf.task(
    "inject_frontmatter",
    command({
      cmd:
        "set -eo pipefail\n" +
        "OUT=$(cat .newton/plan/.planner-output-path)\n" +
        "BRANCH=$(cat .newton/plan/.planner-branch)\n" +
        "tmp=$(mktemp)\n" +
        "{\n" +
        '  echo "---"\n' +
        '  echo "issue: ${ISSUE}"\n' +
        '  echo "branch: ${BRANCH}"\n' +
        '  echo "board_item_id: ${BOARD_ITEM_ID}"\n' +
        '  echo "source: github-project"\n' +
        '  echo "---"\n' +
        '  echo ""\n' +
        '  cat "$OUT"\n' +
        '} > "$tmp"\n' +
        'mv -f "$tmp" "$OUT"\n',
      shell: true,
      captureStdout: false,
      env: {
        ISSUE: wf.input.board_issue_number,
        BOARD_ITEM_ID: wf.input.board_item_id,
      },
    })
  );

  // ----------------------------------------------------------------
  // update_board_body
  // ----------------------------------------------------------------
  const updateBoardBody = wf.task(
    "update_board_body",
    command({
      cmd:
        "set -e\n" +
        'if [[ "$ITEM_ID" == DI_* ]]; then\n' +
        '  gh project item-edit --id "$ITEM_ID" --title "$ITEM_TITLE" --body "$(cat "$OUT")"\n' +
        "else\n" +
        '  gh issue edit "$ISSUE_NUMBER" --body "$(cat "$OUT")"\n' +
        "fi\n",
      shell: true,
      captureStdout: false,
      env: {
        ITEM_ID: wf.input.board_content_id,
        ITEM_TITLE: wf.input.board_item_title,
        ISSUE_NUMBER: wf.input.board_issue_number,
        OUT: readOutputPath.out.stdout,
      },
    })
  );

  // ----------------------------------------------------------------
  // move_to_backlog
  // ----------------------------------------------------------------
  const moveToBacklog = wf.task(
    "move_to_backlog",
    gh.projectItemSetStatus({
      itemId: wf.input.board_item_id,
      board: resolveBoardIds.output,
      status: "Backlog",
      onError: "fail",
    })
  );

  // ----------------------------------------------------------------
  // success (terminal)
  // ----------------------------------------------------------------
  const success = wf.task(
    "success",
    command({
      cmd: 'echo "Planner completed; item moved to Backlog"',
      shell: true,
      captureStdout: false,
    })
  );
  success._terminal = "success";

  // ----------------------------------------------------------------
  // Wire transitions
  // ----------------------------------------------------------------
  resolveBoardIds.then(computePaths);
  computePaths.then(readOutputPath);
  readOutputPath.then(invokeEnricher);
  invokeEnricher.then(injectFrontmatter);
  injectFrontmatter.then(updateBoardBody);
  updateBoardBody.then(moveToBacklog);
  moveToBacklog.then(success);

  return wf;
}

describe("planner round-trip", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("compiles without throwing CompilerError", () => {
    const wf = buildPlanner();
    const yaml = wf.toYaml();
    expect(yaml).toBeTruthy();
  });

  it("is semantically equal to the conformance fixture", () => {
    const wf = buildPlanner();
    const yamlStr = wf.toYaml();
    const compiled = Y.parse(yamlStr) as Record<string, unknown>;
    const expected = loadFixture("planner");

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
});

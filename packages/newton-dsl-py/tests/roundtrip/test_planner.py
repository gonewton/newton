"""
Round-trip test for planner.yaml.

Authors the workflow in Python using the newton-dsl, compiles to YAML,
validates with newton, and asserts semantic equality with the conformance fixture.

planner.yaml is the orchestrator workflow:
- Uses WorkflowOperator (sub_workflow) via invoke_enricher task
- Uses GhOperator: project_resolve_board, project_item_set_status
- Uses CommandOperator with shell scripts
- Context preamble with large prompt template
- No loops — simple linear graph
- max_workflow_iterations: 15
"""
from __future__ import annotations

import warnings

import pytest
import yaml

from newton import Workflow, agent, command, gh, sub_workflow, when, expr
from newton.refs import AmbientRef

from .conftest import (
    validate_with_newton,
    load_fixture,
    tasks_semantic_equal,
    normalize_yaml,
)

PREAMBLE = (
    "You are enriching a product/spec document so that requirements are clearly and explicitly defined "
    "and implementation-ready. The user provided a spec (path or content below).\n\n"
    "CODEBASE GROUNDING (do this first):\n"
    "Before writing the enriched document, you MUST explore the target codebase (read key files, "
    "list directories) to ground every section in the actual project structure. File paths in the "
    '"Modified files" table MUST correspond to real files. If the spec references entities '
    "(commands, endpoints, modules), enumerate them from the source, not from the user's prose.\n\n"
    "You MUST produce an enriched markdown document that MANDATORILY covers these aspects in this "
    "order (use the exact section headers):\n\n"
    "TECHNICAL CONTEXT (sections 1-7):\n"
    "1. **Problem statement** - What is broken or missing today. One paragraph grounded in actual "
    "codebase state. MUST include an explicit \"out of scope\" sentence for related work not in this spec.\n"
    "2. **Goals** - Numbered list of concrete outcomes. Each goal MUST be verifiable.\n"
    "3. **Current behavior** - Table: Component | Role | Mutable at runtime? Plus 1-2 sentences on "
    "current build and resume flow. Grounded in code inspection.\n"
    "4. **Design** - The core \"how\". Subsections for each major mechanism (data structures, "
    "concurrency model, API surface, integration points). MUST include: storage types and data structure "
    "choices with concrete types, concurrency/locking strategy, invariants, identity/naming conventions, "
    "collision and error conditions, tick/lifecycle visibility rules. Include YAML or config examples "
    "where applicable.\n"
    "5. **Schema and API** - Exact struct/type definitions and method signatures that implementers will "
    "create or modify. Use the project's language (Rust, Python, etc.) for code blocks.\n"
    "6. **Error codes** - Table: Code | Category | Trigger. One row per error the implementation "
    "introduces.\n"
    "7. **Acceptance criteria** - Numbered, testable conditions. Each criterion MUST be a single "
    "verifiable statement (not a vague goal). Produce at least one criterion per goal, one per error "
    "code, and one for backward-compatibility regression.\n\n"
    "PLANNING (sections 8-14):\n"
    "8. **Functionality comparison (before vs after)** - What exists today vs what will exist after "
    "implementation.\n"
    "9. **Benefit of implementing this functionality** - Why implement this; value and impact.\n"
    "10. **Stages** - Break the work into ordered stages. Each stage MUST define: (a) a concrete "
    "deliverable or output, (b) the explicit scope (enumerate items, NEVER use \"etc.\" or ellipsis), "
    "and (c) any blocking dependency on a prior stage. If a stage produces a document or artifact "
    "that later stages consume, describe its expected content or structure.\n"
    "11. **Breaking changes** - API, config, or behavioral changes that affect existing users or callers.\n"
    "12. **Modified files and summary** - A table: | File/path | Summary of modifications |. Every "
    "path MUST be verified against the actual codebase. If existing tests or configs already cover "
    "the area, reference them.\n"
    "13. **Dependencies** - External or internal dependencies (libraries, services, teams). For each "
    "dependency on a person or team, state whether it is blocking or non-blocking and which stage it "
    "gates. If a dependency is marked \"recommended\" or \"optional\", state what happens if it is "
    "skipped.\n"
    "14. **Proposed folder structure** - If this is a new project, write complete folder structure; "
    "otherwise only modified files and paths.\n\n"
    "VERIFICATION AND REFERENCE (sections 15-17):\n"
    "15. **Design decisions** - Table: # | Question | Decision | Rationale. Captures key trade-offs "
    "so implementers do not re-debate them.\n"
    "16. **Out of scope (explicit)** - Bulleted list of things explicitly excluded. Prevents scope creep.\n"
    "17. **References** - Pointers to real files, functions, and structs in the codebase that the "
    "implementer should start from.\n\n"
    "Requirements quality (apply throughout the spec):\n"
    "- State requirements in normative language where applicable: MUST (mandatory), SHOULD "
    "(recommended), MAY (optional). Avoid vague wording; make obligations and options explicit.\n"
    "- For each major capability or command, make applicability explicit (e.g. which commands get "
    "which options, or \"all read-only commands\" with a defined list).\n"
    '- NEVER use "etc.", "and so on", or ellipsis to scope work items. If a stage or requirement '
    "applies to a subset of entities, enumerate them explicitly. If the full list is unknown, flag "
    "it with [NEED_USER_INPUT].\n"
    "- Include test expectations where relevant (e.g. \"Format selection MUST be covered by tests "
    "for each updated command\").\n"
    "- Ensure success criteria are measurable or verifiable. When a criterion uses a comparative "
    "metric (e.g. \"2x longer\", \"50% faster\"), it MUST include or reference a concrete baseline "
    "value so the criterion can be checked.\n"
    "- If the spec defines options or formats (e.g. output formats, flags), specify per-entity "
    "applicability or a clear rule so implementers know exactly where each option applies.\n\n"
    "For each of the 17 aspects: if you can fill it from the spec, codebase inspection, or "
    "well-grounded inference, write the section. If you are inventing names, paths, or details that "
    "cannot be verified from the spec or codebase, write exactly \"[NEED_USER_INPUT: <aspect_name>]\" "
    "in that section and add the aspect_name to the gaps list. Valid aspect_name values: "
    "problem_statement, goals, current_behavior, design, schema_api, error_codes, acceptance_criteria, "
    "functionality_comparison, benefit, stages, breaking_changes, modified_files_table, dependencies, "
    "folder_structure, design_decisions, out_of_scope, references.\n\n"
    "When presenting multiple options to the user, present a recommended one and the rationale."
)


def build_planner() -> Workflow:
    """Author planner.yaml using the newton-dsl."""
    wf = Workflow(
        "planner-workflow",
        description=(
            "Orchestrator: resolve board, slug paths; copy planning_enriching into workspace "
            ".newton/workflows; WorkflowOperator loads it via triggers.workspace (nested sandbox); "
            "inject frontmatter; publish issue body; move item to Backlog."
        ),
        parallel_limit=1,
        max_time_seconds=999999999,
        continue_on_error=False,
        max_task_iterations=3,
        max_workflow_iterations=15,
        allow_shell=True,
        default_engine="codex",
    )

    wf.inputs(
        raw_spec_path="",
        board_item_id="",
        board_content_id="",
        board_issue_number="",
        board_item_title="",
    )

    wf.set_context(preamble=PREAMBLE)

    wf.expects("develop_primary_engine", "develop_primary_model")

    # ----------------------------------------------------------------
    # resolve_board_ids
    # ----------------------------------------------------------------
    resolve_board_ids = wf.task(
        "resolve_board_ids",
        gh.project_resolve_board(
            owner=expr('env("GH_PROJECT_OWNER")'),
            project_number=expr('env("GH_PROJECT_NUMBER")'),
            required_option_names=["Backlog"],
        ),
    )

    # ----------------------------------------------------------------
    # compute_paths
    # ----------------------------------------------------------------
    compute_paths = wf.task(
        "compute_paths",
        command(
            (
                "set -eo pipefail\n"
                "slug=$(printf '%s' \"${TITLE:-}\" | iconv -f utf-8 -t ascii//TRANSLIT 2>/dev/null || "
                "printf '%s' \"${TITLE:-}\")\n"
                "slug=$(printf '%s' \"$slug\" | tr '[:upper:]' '[:lower:]' | "
                "sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')\n"
                "if [ -z \"$slug\" ]; then slug=\"spec\"; fi\n"
                "slug=$(printf '%s' \"$slug\" | cut -c1-60 | sed -E 's/-+$//')\n"
                "mkdir -p .newton/plan tmp\n"
                "OUT=\"tmp/${ISSUE}-${slug}.md\"\n"
                "BRANCH=\"feature/${ISSUE}-${slug}\"\n"
                "printf '%s\\n' \"$OUT\" > .newton/plan/.planner-output-path\n"
                "printf '%s\\n' \"$BRANCH\" > .newton/plan/.planner-branch\n"
                "echo \"output_path=$OUT branch=$BRANCH\"\n"
            ),
            shell=True,
            capture_stdout=True,
            env={
                "ISSUE": wf.input.board_issue_number,
                "TITLE": wf.input.board_item_title,
            },
        ),
    )

    # ----------------------------------------------------------------
    # read_output_path
    # ----------------------------------------------------------------
    read_output_path = wf.task(
        "read_output_path",
        command(
            "set -eo pipefail\nprintf '%s' \"$(cat .newton/plan/.planner-output-path)\"\n",
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # invoke_enricher (WorkflowOperator / sub_workflow)
    # ----------------------------------------------------------------
    invoke_enricher = wf.task(
        "invoke_enricher",
        sub_workflow(
            workflow_path=expr('triggers.workspace + "/.newton/workflows/planning_enriching.yaml"'),
            triggers={
                "prompt": wf.input.raw_spec_path,
                "output_path": read_output_path.out.stdout,
            },
            context={
                "preamble": wf.context.preamble,
                "develop_primary_engine": AmbientRef("develop_primary_engine"),
                "develop_primary_model": AmbientRef("develop_primary_model"),
            },
        ),
    )

    # ----------------------------------------------------------------
    # inject_frontmatter
    # ----------------------------------------------------------------
    inject_frontmatter = wf.task(
        "inject_frontmatter",
        command(
            (
                "set -eo pipefail\n"
                "OUT=$(cat .newton/plan/.planner-output-path)\n"
                "BRANCH=$(cat .newton/plan/.planner-branch)\n"
                "tmp=$(mktemp)\n"
                "{\n"
                '  echo "---"\n'
                '  echo "issue: ${ISSUE}"\n'
                '  echo "branch: ${BRANCH}"\n'
                '  echo "board_item_id: ${BOARD_ITEM_ID}"\n'
                '  echo "source: github-project"\n'
                '  echo "---"\n'
                '  echo ""\n'
                '  cat "$OUT"\n'
                '} > "$tmp"\n'
                'mv -f "$tmp" "$OUT"\n'
            ),
            shell=True,
            capture_stdout=False,
            env={
                "ISSUE": wf.input.board_issue_number,
                "BOARD_ITEM_ID": wf.input.board_item_id,
            },
        ),
    )

    # ----------------------------------------------------------------
    # update_board_body
    # ----------------------------------------------------------------
    update_board_body = wf.task(
        "update_board_body",
        command(
            (
                "set -e\n"
                "if [[ \"$ITEM_ID\" == DI_* ]]; then\n"
                '  gh project item-edit --id "$ITEM_ID" --title "$ITEM_TITLE" --body "$(cat "$OUT")"\n'
                "else\n"
                '  gh issue edit "$ISSUE_NUMBER" --body "$(cat "$OUT")"\n'
                "fi\n"
            ),
            shell=True,
            capture_stdout=False,
            env={
                "ITEM_ID": wf.input.board_content_id,
                "ITEM_TITLE": wf.input.board_item_title,
                "ISSUE_NUMBER": wf.input.board_issue_number,
                "OUT": read_output_path.out.stdout,
            },
        ),
    )

    # ----------------------------------------------------------------
    # move_to_backlog
    # ----------------------------------------------------------------
    move_to_backlog = wf.task(
        "move_to_backlog",
        gh.project_item_set_status(
            item_id=wf.input.board_item_id,
            board=resolve_board_ids.output,
            status="Backlog",
            on_error="fail",
        ),
    )

    # ----------------------------------------------------------------
    # success (terminal)
    # ----------------------------------------------------------------
    success = wf.task(
        "success",
        command(
            'echo "Planner completed; item moved to Backlog"',
            shell=True,
            capture_stdout=False,
        ),
    )
    success._terminal = "success"

    # ----------------------------------------------------------------
    # Wire transitions
    # ----------------------------------------------------------------
    resolve_board_ids.then(compute_paths)
    compute_paths.then(read_output_path)
    read_output_path.then(invoke_enricher)
    invoke_enricher.then(inject_frontmatter)
    inject_frontmatter.then(update_board_body)
    update_board_body.then(move_to_backlog)
    move_to_backlog.then(success)

    return wf


def test_planner_compiles() -> None:
    """The planner workflow compiles without error."""
    wf = build_planner()
    with warnings.catch_warnings(record=True):
        warnings.simplefilter("always")
        yaml_str = wf.to_yaml()
    assert yaml_str, "to_yaml() returned empty string"


def test_planner_validates() -> None:
    """Compiled YAML passes newton workflow validate."""
    wf = build_planner()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    ok, output = validate_with_newton(yaml_str)
    assert ok, f"newton validate failed:\n{output}\n\nYAML:\n{yaml_str}"


def test_planner_semantic_equality() -> None:
    """Compiled YAML is semantically equal to the conformance fixture."""
    wf = build_planner()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    compiled = yaml.safe_load(yaml_str)
    expected = load_fixture("planner")

    # version, mode
    assert compiled.get("version") == expected.get("version"), "version mismatch"
    assert compiled.get("mode") == expected.get("mode"), "mode mismatch"

    # metadata
    compiled_name = (compiled.get("metadata") or {}).get("name")
    expected_name = (expected.get("metadata") or {}).get("name")
    assert compiled_name == expected_name, f"metadata.name: {compiled_name!r} != {expected_name!r}"

    # settings
    compiled_settings = normalize_yaml(compiled["workflow"]["settings"])
    expected_settings = normalize_yaml(expected["workflow"]["settings"])
    for key in ["entry_task", "parallel_limit", "continue_on_error", "max_task_iterations",
                "max_workflow_iterations"]:
        assert compiled_settings.get(key) == expected_settings.get(key), (
            f"settings.{key}: {compiled_settings.get(key)!r} != {expected_settings.get(key)!r}"
        )

    # tasks
    ok, diff = tasks_semantic_equal(
        compiled["workflow"]["tasks"],
        expected["workflow"]["tasks"],
    )
    assert ok, f"Task semantic mismatch:\n{diff}"

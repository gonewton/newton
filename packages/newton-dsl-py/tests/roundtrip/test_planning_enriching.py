"""
Round-trip test for planning_enriching.yaml.

Authors the workflow in Python, compiles to YAML, validates with newton,
and asserts semantic equality with the conformance fixture.

Also verifies that:
- The compiler warns about task-1 (unreachable)
- A workflow with an unbounded cycle raises CompilerError
- A workflow with a dangling edge raises CompilerError
"""
from __future__ import annotations

import warnings

import pytest
import yaml

from newton import Workflow, agent, command, when, expr
from newton.checks import CompilerError
from newton.refs import AmbientRef

from .conftest import (
    validate_with_newton,
    load_fixture,
    tasks_semantic_equal,
    normalize_yaml,
)


def build_planning_enriching() -> Workflow:
    """
    Author planning_enriching.yaml in Python using the newton-dsl.
    """
    wf = Workflow(
        "planning-enriching",
        description=(
            "Pure spec enrichment (no GitHub). Invoked via WorkflowOperator from planner.yaml, "
            "or standalone: newton run planning_enriching.yaml --arg prompt=... --arg output_path=... (--set engines)."
        ),
        default_engine="codex",
        parallel_limit=1,
        max_time_seconds=999999999,
        continue_on_error=False,
        max_task_iterations=3,
        max_workflow_iterations=15,
        allow_shell=True,
    )

    wf.inputs(prompt="", output_path="")
    wf.expects("develop_primary_engine", "develop_primary_model")

    # Ambient refs for injected vars
    engine = AmbientRef("develop_primary_engine")
    model = AmbientRef("develop_primary_model")

    # ----------------------------------------------------------------
    # enrich_spec
    # ----------------------------------------------------------------
    enrich_prompt = expr(
        'context.preamble + "\\n\\nOutput format:\\n'
        '- Write the full enriched markdown to the file " + triggers.output_path + " in the workspace (create directory if needed).\\n'
        '- Write the list of aspect names that need user input (one per line) to .newton/plan/gaps.txt. If none need input, write a single line \\"none\\". '
        'Use only these aspect names when listing gaps: problem_statement, goals, current_behavior, design, schema_api, error_codes, acceptance_criteria, '
        'functionality_comparison, benefit, stages, breaking_changes, modified_files_table, dependencies, folder_structure, design_decisions, out_of_scope, references.\\n\\n'
        '---\\nUser spec (path or content):\\n\\n" + triggers.prompt'
    )

    enrich_spec = wf.task(
        "enrich_spec",
        agent(
            engine=engine,
            model=model,
            prompt=enrich_prompt,
        ),
    )
    enrich_spec.repeat_at_most(2)

    # ----------------------------------------------------------------
    # check_gaps
    # ----------------------------------------------------------------
    check_gaps_cmd = (
        "set -e\n"
        'if [ ! -f "$OUT" ]; then\n'
        '  echo "check_gaps: enriched spec missing at ${OUT} (enrich agent must create this path)" >&2\n'
        "  exit 1\n"
        "fi\n"
        'if grep -q "NEED_USER_INPUT" "$OUT"; then\n'
        '  printf "has_gaps"\n'
        "else\n"
        '  printf "no_gaps"\n'
        "fi\n"
    )

    check_gaps = wf.task(
        "check_gaps",
        command(
            check_gaps_cmd,
            shell=True,
            env={"OUT": wf.input.output_path},
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # cat_gaps
    # ----------------------------------------------------------------
    cat_gaps = wf.task(
        "cat_gaps",
        command(
            "cat .newton/plan/gaps.txt 2>/dev/null || echo 'none'",
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # clarify_spec
    # ----------------------------------------------------------------
    clarify_prompt = expr(
        '"You are running the newton-clarify-question protocol for a spec that still has unresolved gaps.\\n\\n'
        'Spec path: " + triggers.output_path + "\\nGaps file: .newton/plan/gaps.txt\\n\\n'
        'Steps:\\n1. Read the spec and collect every NEED_USER_INPUT tag with surrounding context.\\n'
        '2. For each gap, ground at least two concrete alternatives in the actual codebase, lockfiles, and dependencies. '
        'Prefer evidence over guesses; list unverifiable claims in evidence_gaps.\\n'
        '3. Pick a recommended alternative (primary_alternative_id must match one of alternatives[].id).\\n\\n'
        'Output a single ```json fenced code block containing one object that validates against this schema (schema_version must be 1):\\n'
        '{\\n  \\"schema_version\\": 1,\\n  \\"spec_paths\\": [\\"<" + triggers.output_path + ">\\"],\\n'
        '  \\"need_user_input\\": [{\\"tag\\": string, \\"excerpt\\": string, \\"location_hint\\": string}],\\n'
        '  \\"context_notes\\": [string],\\n  \\"evidence_gaps\\": [string],\\n  \\"problem_statement\\": string,\\n'
        '  \\"alternatives\\": [{\\"id\\": \\"<slug: ^[a-z][a-z0-9_-]*$>\\", \\"title\\": string, \\"pros\\": [string], \\"cons\\": [string]}],\\n'
        '  \\"recommendation\\": {\\"summary\\": string, \\"rationale\\": string, \\"primary_alternative_id\\": string, \\"confidence\\": \\"low\\"|\\"medium\\"|\\"high\\"}\\n'
        '}\\n\\nRules:\\n- alternatives must have at least 2 entries\\n'
        '- alternatives[].id must be lowercase slugs (^[a-z][a-z0-9_-]*$)\\n'
        '- recommendation.primary_alternative_id must equal one of the alternatives[].id values\\n'
        '- Do NOT include defer or abort alternatives; the workflow adds those\\n\\n'
        'After producing the JSON block, write the JSON object (not the markdown mirror) to .newton/plan/clarify.json."'
    )

    clarify_spec = wf.task(
        "clarify_spec",
        agent(
            engine=engine,
            model=model,
            prompt=clarify_prompt,
        ),
    )
    clarify_spec.repeat_at_most(2)

    # ----------------------------------------------------------------
    # validate_clarify
    # ----------------------------------------------------------------
    validate_cmd = (
        "set -e\n"
        'CLARIFY=".newton/plan/clarify.json"\n'
        'if [ ! -f "$CLARIFY" ]; then\n'
        '  echo "ERROR: clarify.json not written by clarify_spec agent" >&2\n'
        "  exit 1\n"
        "fi\n"
        "jq -e '\n"
        "  .schema_version == 1\n"
        "  and (.spec_paths | length > 0)\n"
        "  and (.need_user_input | length > 0)\n"
        "  and (.problem_statement | length > 0)\n"
        "  and (.alternatives | length >= 2)\n"
        "  and (.recommendation.primary_alternative_id != null)\n"
        "  and (\n"
        "    .recommendation.primary_alternative_id as $rid\n"
        "    | [.alternatives[].id] | contains([$rid])\n"
        "  )\n"
        "' \"$CLARIFY\" > /dev/null\n"
    )

    validate_clarify = wf.task(
        "validate_clarify",
        command(
            validate_cmd,
            shell=True,
            capture_stdout=False,
        ),
    )

    # ----------------------------------------------------------------
    # present_decision
    # ----------------------------------------------------------------
    present_cmd = (
        "set -e\n"
        'CLARIFY=".newton/plan/clarify.json"\n'
        "PAYLOAD=$(jq -c '{\n"
        "  decision_id: (\"planning-enriching-\" + (now | floor | tostring)),\n"
        "  summary: .problem_statement,\n"
        "  context_markdown: (\n"
        '    "**Decision required for:** " + .problem_statement\n'
        '    + "\\n\\n**Open gaps:**\\n"\n'
        '    + (.need_user_input | map("- **\\(.tag)**: \\(.excerpt) _(\\(.location_hint))_") | join("\\n"))\n'
        "  ),\n"
        "  options: (\n"
        "    (.alternatives | map({\n"
        "      id: .id,\n"
        "      label: .title,\n"
        "      detail_markdown: (\n"
        '        "**Pros:** " + (.pros | join(" · "))\n'
        '        + "\\n\\n**Cons:** " + (.cons | join(" · "))\n'
        "      )\n"
        "    }))\n"
        "    + [\n"
        '      {"id": "defer", "label": "Defer — keep placeholders, revisit later"},\n'
        '      {"id": "abort", "label": "Abort — stop enrichment without merging"}\n'
        "    ]\n"
        "  ),\n"
        "  recommendation: {\n"
        "    option_id: .recommendation.primary_alternative_id,\n"
        "    rationale_markdown: (\n"
        "      .recommendation.rationale\n"
        '      + ((.recommendation.confidence // "") | if . != "" then " *(confidence: " + . + ")*" else "" end)\n'
        "    )\n"
        "  }\n"
        "}' \"$CLARIFY\")\n"
        'CHOICE=$(ailoop ask --payload "$PAYLOAD" --json | jq -r \'.response\')\n'
        'printf "%s" "$CHOICE"\n'
    )

    present_decision = wf.task(
        "present_decision",
        command(
            present_cmd,
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # merge_spec
    # ----------------------------------------------------------------
    merge_prompt = expr(
        '"Read the enriched spec at " + triggers.output_path + " and the clarifier output at .newton/plan/clarify.json.\\n\\n'
        'The human selected option id: \\"" + tasks.present_decision.output.stdout + "\\"\\n\\n'
        'Find that alternative in clarify.json (alternatives[].id == \\"" + tasks.present_decision.output.stdout + "\\"). '
        'If the id is not found (e.g. fallback), use the recommendation instead.\\n\\n'
        'For every NEED_USER_INPUT placeholder in the spec:\\n'
        '- Replace it with a concrete resolution grounded in the chosen alternative (title, pros, cons from clarify.json).\\n'
        '- If a placeholder cannot be fully resolved by the chosen alternative, document it as an explicit design decision or open question — never leave bare NEED_USER_INPUT tags.\\n\\n'
        'Write the resolved spec back to " + triggers.output_path + " (overwrite in place). Leave sections without placeholders unchanged."'
    )

    merge_spec = wf.task(
        "merge_spec",
        agent(
            engine=engine,
            model=model,
            prompt=merge_prompt,
        ),
    )
    merge_spec.repeat_at_most(1)

    # ----------------------------------------------------------------
    # finalize
    # ----------------------------------------------------------------
    finalize = wf.task(
        "finalize",
        command(
            "echo Enriched spec written to $OUT",
            shell=True,
            env={"OUT": wf.input.output_path},
            capture_stdout=False,
        ),
    )
    finalize._terminal = "success"

    # ----------------------------------------------------------------
    # task-1 (dead task — unreachable, compiler should warn)
    # ----------------------------------------------------------------
    from newton.task import Task as _Task
    task1 = _Task("task-1", agent())
    wf._tasks["task-1"] = task1
    wf._task_list.append(task1)

    # ----------------------------------------------------------------
    # Wire transitions
    # ----------------------------------------------------------------
    enrich_spec.then(check_gaps)

    check_gaps.then(
        cat_gaps,
        when=expr('tasks.check_gaps.output.stdout == "has_gaps"'),
    ).then(
        finalize,
        when=expr('tasks.check_gaps.output.stdout == "no_gaps"'),
    )

    cat_gaps.then(clarify_spec)
    clarify_spec.then(validate_clarify)
    validate_clarify.then(present_decision)

    present_decision.then(
        finalize,
        when=expr('tasks.present_decision.output.stdout == "defer"'),
    ).then(
        finalize,
        when=expr('tasks.present_decision.output.stdout == "abort"'),
    ).then(merge_spec)

    merge_spec.then(finalize)

    return wf


def test_planning_enriching_compiles() -> None:
    """The workflow compiles without raising CompilerError."""
    wf = build_planning_enriching()
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        yaml_str = wf.to_yaml()

    assert yaml_str, "to_yaml() returned empty string"

    # task-1 should be flagged as unreachable
    unreachable_warnings = [
        w for w in caught
        if "task-1" in str(w.message) and "unreachable" in str(w.message).lower()
    ]
    assert unreachable_warnings, (
        f"Expected warning about unreachable task-1, got: {[str(w.message) for w in caught]}"
    )


def test_planning_enriching_validates() -> None:
    """Compiled YAML passes newton workflow validate."""
    wf = build_planning_enriching()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    ok, output = validate_with_newton(yaml_str)
    assert ok, f"newton validate failed:\n{output}\n\nYAML:\n{yaml_str}"


def test_planning_enriching_semantic_equality() -> None:
    """
    Compiled YAML is semantically equal to the conformance fixture,
    excluding task-1 (which is dead code in the fixture but validly present).
    """
    wf = build_planning_enriching()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    compiled = yaml.safe_load(yaml_str)
    expected = load_fixture("planning_enriching")

    # Compare top-level fields
    assert compiled.get("version") == expected.get("version"), "version mismatch"
    assert compiled.get("mode") == expected.get("mode"), "mode mismatch"

    # Compare metadata name
    compiled_name = (compiled.get("metadata") or {}).get("name")
    expected_name = (expected.get("metadata") or {}).get("name")
    assert compiled_name == expected_name, f"metadata.name: {compiled_name!r} != {expected_name!r}"

    # Compare workflow settings
    compiled_settings = normalize_yaml(compiled["workflow"]["settings"])
    expected_settings = normalize_yaml(expected["workflow"]["settings"])
    # Check key settings
    for key in ["entry_task", "parallel_limit", "continue_on_error", "max_task_iterations",
                "max_workflow_iterations", "default_engine"]:
        assert compiled_settings.get(key) == expected_settings.get(key), (
            f"settings.{key}: {compiled_settings.get(key)!r} != {expected_settings.get(key)!r}"
        )

    # Compare tasks (semantic equality)
    ok, diff = tasks_semantic_equal(
        compiled["workflow"]["tasks"],
        expected["workflow"]["tasks"],
    )
    assert ok, f"Task semantic mismatch:\n{diff}"


def test_compiler_rejects_unbounded_cycle() -> None:
    """Compiler raises CompilerError for a cycle with no max_iterations."""
    wf = Workflow("test-cycle")
    a = wf.task("a", command("echo a"))
    b = wf.task("b", command("echo b"))
    a.then(b)
    b.then(a)  # back-edge with no cap

    with pytest.raises(CompilerError, match="unbounded cycle"):
        wf.to_yaml()


def test_compiler_rejects_dangling_edge() -> None:
    """Compiler raises CompilerError for an edge to an undefined task."""
    from newton.task import Task as _Task
    from newton.edges import EdgeSpec

    wf = Workflow("test-dangling")
    a = wf.task("a", command("echo a"))

    # Manually inject a dangling edge
    a._edges.append(EdgeSpec("nonexistent", priority=0))

    with pytest.raises(CompilerError, match="undefined task"):
        wf.to_yaml()


def test_compiler_warns_unreachable_task() -> None:
    """Compiler emits a CompilerWarning for tasks unreachable from entry_task."""
    import warnings as _warnings

    wf = Workflow("test-unreachable")
    wf.task("a", command("echo a"))
    wf.task("orphan", command("echo orphan"))  # no edges lead here

    # to_yaml should succeed but warn about 'orphan'
    with _warnings.catch_warnings(record=True) as caught:
        _warnings.simplefilter("always")
        yaml_str = wf.to_yaml()

    assert yaml_str  # compiled successfully
    messages = [str(w.message) for w in caught]
    assert any("orphan" in m for m in messages), (
        f"Expected unreachable warning for 'orphan', got: {messages}"
    )


def test_compiler_rejects_unknown_out_field() -> None:
    """Compiler raises CompilerError when .out.field is not in the operator's output schema."""
    wf = Workflow("test-out-field")
    t = wf.task("cmd", command("echo hi"))

    with pytest.raises(CompilerError, match="no output field"):
        _ = t.out.nonexistent_field

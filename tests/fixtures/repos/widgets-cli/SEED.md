# widgets-cli — Seed Oracle

## Starting state

Four seeded maintainability issues are present in the initial codebase.
The optimization loop should detect and fix all four, leaving only
behaviour-neutral refactors with all pytest tests still green.

## Seeded issues

| # | File | Issue | Dimension | Fix |
|---|------|-------|-----------|-----|
| A | `src/widgets/inventory.py` | `process()` is a ~90-line god-function mixing validation, persistence stubs, event hooks, and index updates | `file_decomposition` | Split into focused helpers: `_validate_add`, `_do_add`, `_validate_remove`, `_do_remove` |
| B | `src/widgets/formatting.py` + `src/widgets/report.py` | `print_table()` is copy-pasted verbatim in both files | `canonical_placement` | Extract to `formatting.py` (canonical), import in `report.py` |
| C | `src/widgets/inventory.py` | `validate_unused()` stub is defined but never called anywhere | `abstraction_economy` | Delete the function |
| D | `src/widgets/cli.py` | Command dispatch uses `cmd.find("add") != -1` substring matching | `branching_discipline` | Replace with exact-match dict dispatch |

## Grading oracle

Starting score band: 55–65 (all four issues present).
Convergence target: Grade ≥ 85, zero high/medium open Findings.
Each issue resolved adds ≈10 points to the overall score.

## Deterministic grader contract

The seed grader (`tests/fixtures/graders/seed-grader.sh`) detects issues
by structural checks:
- A: `process()` non-blank line count > 40
- B: `def print_table` appears in both `formatting.py` AND `report.py`
- C: `validate_unused` defined in `inventory.py`
- D: `cmd.find(` appears in `cli.py`

The grader emits a Newton Assessment JSON to stdout.
Each resolved issue removes its Observation and improves its dimension Score.

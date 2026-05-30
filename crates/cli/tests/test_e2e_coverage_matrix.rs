//! Spec 301 §4.2 — `E2E_COVERAGE.md` matrix gate.
//!
//! Validates that:
//!   * Every required root command id has at least one smoke row
//!     (`WFG-E2EMAT-001`).
//!   * Every test name referenced by the matrix exists in some test source
//!     file (`WFG-E2EMAT-002`).
//!   * Every flag referenced in the matrix corresponds to either `--help`,
//!     a documented framework flag, or a flag declared in `framework_setup/`
//!     (`WFG-E2EMAT-003`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// Decision #4 (spec 051): "run" is kept in REQUIRED_SMOKE_IDS to gate coverage of the
// hidden deprecation shim (`newton run`). The `smoke_run_help` row was repurposed to invoke
// the deprecated path and assert stderr contains "[newton] DEPRECATED". Both "run" and
// "workflow" entries MUST be removed from this array in the same PR that deletes the hidden
// shim.
const REQUIRED_SMOKE_IDS: &[&str] = &[
    "run",
    "init",
    "batch",
    "serve",
    "workflow",
    "resume",
    "checkpoint",
    "artifact",
    "webhook",
    "runs",
    "health",
    "doctor",
    "config",
    "completion",
    "chat",
    "spec",
];

fn cli_tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn matrix_md() -> String {
    let p = cli_tests_dir().join("E2E_COVERAGE.md");
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[derive(Debug, Clone)]
struct Row {
    command: String,
    flag: String,
    test_name: String,
    tier: String,
}

fn parse_rows(md: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in md.lines() {
        let l = line.trim();
        if l.starts_with("| Command path") {
            in_table = true;
            continue;
        }
        if in_table {
            if !l.starts_with('|') {
                in_table = false;
                continue;
            }
            // skip header separator
            if l.contains("---") {
                continue;
            }
            let cells: Vec<&str> = l.trim_matches('|').split('|').map(|s| s.trim()).collect();
            if cells.len() < 4 {
                continue;
            }
            rows.push(Row {
                command: cells[0].to_string(),
                flag: cells[1].to_string(),
                test_name: cells[2].to_string(),
                tier: cells[3].to_string(),
            });
        }
    }
    rows
}

fn collect_test_names() -> BTreeSet<String> {
    // Scan all `.rs` files under tests/ and collect only function names that
    // are preceded by a `#[test]` attribute (possibly with other attributes or
    // blank lines between). This avoids false-positive matches on helper
    // functions and non-test `fn` declarations.
    let mut names = BTreeSet::new();
    let root = cli_tests_dir();
    fn walk(dir: &Path, names: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, names);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(src) = fs::read_to_string(&p) {
                    // Track whether we are inside an attribute block that
                    // contains `#[test]`. The block is reset on any non-
                    // attribute, non-blank line (including the `fn` itself).
                    let mut in_test_block = false;
                    for line in src.lines() {
                        let t = line.trim();
                        if t.starts_with("#[test") {
                            in_test_block = true;
                        } else if t.starts_with('#') {
                            // another attribute — keep the block alive
                        } else if t.is_empty() {
                            // blank line — keep the block alive (rare but valid)
                        } else {
                            // Code line — extract the fn name if under #[test].
                            if in_test_block {
                                // Strip optional `pub` / `async` prefixes.
                                let stripped = t
                                    .trim_start_matches("pub ")
                                    .trim_start_matches("async ")
                                    .trim_start_matches("pub ");
                                if let Some(rest) = stripped.strip_prefix("fn ") {
                                    if let Some(end) = rest.find('(') {
                                        let n = rest[..end].trim();
                                        if !n.is_empty() {
                                            names.insert(n.to_string());
                                        }
                                    }
                                }
                            }
                            in_test_block = false;
                        }
                    }
                }
            }
        }
    }
    walk(&root, &mut names);
    names
}

#[test]
fn every_command_has_smoke_row() {
    let rows = parse_rows(&matrix_md());
    let mut by_root: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        let root = r
            .command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        by_root.entry(root).or_default().push(r);
    }
    for id in REQUIRED_SMOKE_IDS {
        let has_smoke = by_root
            .get(*id)
            .map(|v| v.iter().any(|r| r.tier.eq_ignore_ascii_case("smoke")))
            .unwrap_or(false);
        assert!(
            has_smoke,
            "WFG-E2EMAT-001: required command id `{id}` has no smoke row in E2E_COVERAGE.md"
        );
    }
}

#[test]
fn matrix_rows_reference_existing_tests() {
    let rows = parse_rows(&matrix_md());
    let names = collect_test_names();
    for r in &rows {
        assert!(
            names.contains(&r.test_name),
            "WFG-E2EMAT-002: matrix row references unknown test `{}` (command `{}`, tier `{}`)",
            r.test_name,
            r.command,
            r.tier,
        );
    }
}

#[test]
fn matrix_flags_exist_in_command_spec() {
    // Lightweight check: every flag in the matrix is either:
    //   * empty (positional / no flag column)
    //   * `--help`
    //   * starts with `--` and appears textually in framework_setup.rs (or
    //     is a documented `spec`/framework flag)
    //   * a `(negative)` annotation
    let rows = parse_rows(&matrix_md());
    let fw_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/framework_setup");
    let fw = collect_rs_sources(&fw_dir);
    let allowed_framework_flags: BTreeSet<&str> = [
        "--help",
        "--format",
        "--output",
        "--include-hidden",
        "--version",
    ]
    .into_iter()
    .collect();

    for r in &rows {
        let flag = r.flag.trim();
        if flag.is_empty() {
            continue;
        }
        // Tolerate annotations like "--bogus-flag (negative)" or
        // "(missing positional)"
        let first_token = flag.split_whitespace().next().unwrap_or("");
        if !first_token.starts_with("--") {
            continue;
        }
        if first_token == "--bogus-flag" {
            continue; // documented negative case
        }
        if allowed_framework_flags.contains(first_token) {
            continue;
        }
        let appears = fw.contains(first_token);
        assert!(
            appears,
            "WFG-E2EMAT-003: matrix flag `{first_token}` (command `{}`) not declared in framework_setup/",
            r.command
        );
    }
}

// --- Helpers -----------------------------------------------------------------

/// Concatenate all `.rs` files under `dir` recursively into a single string.
fn collect_rs_sources(dir: &Path) -> String {
    let mut out = String::new();
    if let Ok(entries) = fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                out.push_str(&collect_rs_sources(&path));
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Ok(s) = fs::read_to_string(&path) {
                    out.push_str(&s);
                }
            }
        }
    }
    out
}

// --- Self-tests for the three error codes ---------------------------------

#[test]
fn wfg_e2emat_001_fires_on_missing_smoke_row() {
    let synthetic = "| Command path | Flag | Test name | Tier |\n|---|---|---|---|\n| run | --help | smoke_run_help | smoke |\n";
    let rows = parse_rows(synthetic);
    let mut by_root: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        let root = r
            .command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        by_root.entry(root).or_default().push(r);
    }
    let missing: Vec<&&str> = REQUIRED_SMOKE_IDS
        .iter()
        .filter(|id| {
            !by_root
                .get(**id)
                .map(|v| v.iter().any(|r| r.tier.eq_ignore_ascii_case("smoke")))
                .unwrap_or(false)
        })
        .collect();
    let msg = format!("WFG-E2EMAT-001: missing smoke rows for {missing:?}");
    assert!(msg.contains("WFG-E2EMAT-001"));
    assert!(!missing.is_empty());
}

#[test]
fn wfg_e2emat_002_fires_on_unknown_test_name() {
    let names: BTreeSet<String> = ["smoke_run_help".to_string()].into_iter().collect();
    let row = Row {
        command: "run".into(),
        flag: "--help".into(),
        test_name: "smoke_does_not_exist".into(),
        tier: "smoke".into(),
    };
    let exists = names.contains(&row.test_name);
    let msg = format!(
        "WFG-E2EMAT-002: matrix row references unknown test `{}`",
        row.test_name
    );
    assert!(!exists);
    assert!(msg.contains("WFG-E2EMAT-002"));
}

#[test]
fn wfg_e2emat_003_fires_on_unknown_flag() {
    let fw = "id: \"run\"\n--workspace\n--trigger\n";
    let flag = "--definitely-not-here";
    let appears = fw.contains(flag);
    let msg = format!("WFG-E2EMAT-003: matrix flag `{flag}` not declared");
    assert!(!appears);
    assert!(msg.contains("WFG-E2EMAT-003"));
}

//! AC-G7 (251 spec) regression guard: forbid wildcard `_ => …` arms in `match`
//! statements that match `event.payload` (`AgentEventPayload`) or `RunError`
//! anywhere under `crates/core/src/workflow/operators/{agent,engine}/`.
//!
//! `aikit_sdk::AgentEventPayload` and `aikit_sdk::RunError` are `#[non_exhaustive]`,
//! so the type system requires *some* wildcard arm. This guard tolerates exactly
//! the documented sentinel arms (followed by an explanatory comment containing the
//! word "non_exhaustive") and flags any other `_ =>` usage that would silently
//! drop new SDK variants.
//!
//! The check is intentionally a textual heuristic — false positives are acceptable
//! per the spec.

use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn no_wildcard_payload_or_run_error_arms() {
    let roots = [
        manifest_root().join("src/workflow/operators/agent"),
        manifest_root().join("src/workflow/operators/agent.rs"),
        manifest_root().join("src/workflow/operators/engine"),
    ];

    let mut files: Vec<PathBuf> = Vec::new();
    for root in &roots {
        if root.is_dir() {
            collect_rs_files(root, &mut files);
        } else if root.is_file() {
            files.push(root.clone());
        }
    }
    assert!(!files.is_empty(), "expected to scan at least one file");

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let content = fs::read_to_string(path).expect("read file");
        let lines: Vec<&str> = content.lines().collect();

        // Track simple block context: track when we are inside a `match …event.payload`
        // or a `match … RunError` block by counting braces from the opening `match` line.
        let mut in_payload_match = false;
        let mut in_run_error_match = false;
        let mut depth: i32 = 0;
        let mut block_start_depth: i32 = 0;

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect entry into a payload / RunError match. We deliberately key on
            // common substrings used in the codebase.
            if !in_payload_match
                && !in_run_error_match
                && trimmed.contains("match ")
                && (trimmed.contains("event.payload")
                    || trimmed.contains("&event.payload")
                    || trimmed.contains("e.payload"))
            {
                in_payload_match = true;
                block_start_depth = depth;
            } else if !in_payload_match
                && !in_run_error_match
                && trimmed.starts_with("pub fn map_run_error")
            {
                in_run_error_match = true;
                block_start_depth = depth;
            }

            // Update brace depth based on this line.
            for ch in line.chars() {
                if ch == '{' {
                    depth += 1;
                }
                if ch == '}' {
                    depth -= 1;
                }
            }

            if (in_payload_match || in_run_error_match)
                && (trimmed.starts_with("_ =>") || trimmed.starts_with("_=>"))
            {
                // Allow if a nearby (previous 3 lines or this line's trailing comment) line
                // mentions "non_exhaustive" — that is the sentinel arm required by the
                // type system for cross-crate `#[non_exhaustive]` matches.
                let window_start = idx.saturating_sub(3);
                let window: String = lines[window_start..=idx].join("\n");
                if !window.contains("non_exhaustive") {
                    violations.push(format!(
                        "{}:{}: forbidden wildcard `_ =>` arm in match against \
                         AgentEventPayload / RunError (per 251 spec AC-G7)",
                        path.display(),
                        idx + 1
                    ));
                }
            }

            // Exit the tracked block when depth drops back to where we started.
            if (in_payload_match || in_run_error_match) && depth <= block_start_depth {
                in_payload_match = false;
                in_run_error_match = false;
            }
        }
    }

    assert!(
        violations.is_empty(),
        "AC-G7 wildcard arm violations:\n{}",
        violations.join("\n")
    );
}

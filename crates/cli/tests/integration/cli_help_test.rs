use assert_cmd::Command;

const BIN: &str = "newton";

fn help_output(args: &[&str]) -> String {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .args(args)
        .arg("--help")
        .output()
        .expect("should run successfully");

    std::str::from_utf8(&output.stdout).unwrap().to_string()
}

#[test]
fn init_help_has_examples_section() {
    let stdout = help_output(&["init"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "init --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn init_help_shows_current_dir_example() {
    let stdout = help_output(&["init"]);
    assert!(
        stdout.contains("newton init ."),
        "init --help should show current-directory initialization example"
    );
}

#[test]
fn batch_help_has_examples_section() {
    let stdout = help_output(&["batch"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "batch --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn batch_help_shows_project_id_example() {
    let stdout = help_output(&["batch"]);
    assert!(
        stdout.contains("newton batch project-alpha"),
        "batch --help should show project-id usage example"
    );
}

#[test]
fn batch_help_shows_workspace_flag_example() {
    let stdout = help_output(&["batch"]);
    assert!(
        stdout.contains("--workspace"),
        "batch --help should demonstrate --workspace flag"
    );
}

#[test]
fn all_main_commands_have_examples() {
    let commands: &[&[&str]] = &[
        &["init"],
        &["batch"],
        &["serve"],
        &["workflow"],
        &["webhook"],
    ];
    for command in commands {
        let stdout = help_output(command);
        assert!(
            stdout.contains("EXAMPLES:"),
            "{:?} --help should contain an examples section",
            command
        );
    }
}

/// Parity check (spec §10 Stage E / §15 D8): asserts that the post-migration
/// `newton --help` output stays in lock-step with the `help_parity.snap`
/// artifact that the CHANGELOG references.
///
/// The new cli-framework (v0.4.2+) renders grouped-by-category help instead of
/// a flat "Commands:" list. We normalize by:
/// 1. Keeping the "Usage:" header line.
/// 2. Collecting all category-section command lines (first indented token = command name),
///    stripping the Usage: sub-lines (double-indented).
/// 3. Sorting the collected command summary lines within each category section.
/// 4. Keeping "Options:" lines sorted.
///
/// This keeps the snapshot stable across cli-framework iteration-order changes.
#[test]
fn newton_help_matches_parity_snapshot() {
    fn regex_replace_version(s: &str) -> String {
        // Replace "newton X.Y.Z" with "newton VERSION" so option lines are
        // stable across version bumps and the snapshot never needs updating for
        // a release.
        let mut out = String::new();
        let marker = "newton ";
        if let Some(pos) = s.find(marker) {
            let after = &s[pos + marker.len()..];
            let end = after
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(after.len());
            if end > 0 && after[..end].contains('.') {
                out.push_str(&s[..pos]);
                out.push_str("newton VERSION");
                out.push_str(&after[end..]);
                return out;
            }
        }
        s.to_string()
    }

    fn normalize(text: &str) -> String {
        // New grouped format: category headers are "Word:" at column 0 (not
        // "Commands:" or "Options:"). Command lines are single-indented. Usage
        // hint sub-lines are double-indented — we strip those for the snapshot.
        let mut out_lines: Vec<String> = Vec::new();
        let mut current_category: Option<String> = None;
        // category → sorted command lines (first summary line only)
        let mut category_commands: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        let mut options_lines: Vec<String> = Vec::new();
        let mut in_options = false;
        let mut usage_line: Option<String> = None;

        for raw_line in text.lines() {
            let trimmed = raw_line.trim_end();

            // Usage line (first non-empty)
            if usage_line.is_none() && trimmed.starts_with("Usage:") {
                // Normalise binary path: replace everything up to and including the binary name
                let normalised = if let Some(pos) = trimmed.rfind('/') {
                    format!(
                        "Usage: newton{}",
                        &trimmed[pos + trimmed[pos..].find(' ').unwrap_or(trimmed.len() - pos)..]
                    )
                } else {
                    trimmed.to_string()
                };
                usage_line = Some(normalised);
                continue;
            }

            // Options section
            if trimmed == "Options:" {
                in_options = true;
                current_category = None;
                continue;
            }
            if in_options {
                if !trimmed.is_empty() {
                    // Strip the semver token so the snapshot never needs
                    // updating for a version bump: "newton 0.5.111" → "newton VERSION".
                    let normalised_opt = regex_replace_version(trimmed);
                    options_lines.push(normalised_opt);
                }
                continue;
            }

            // Category header: "Word:" at column 0 (no leading whitespace)
            if !raw_line.starts_with(' ') && trimmed.ends_with(':') && !trimmed.is_empty() {
                current_category = Some(trimmed.trim_end_matches(':').to_string());
                continue;
            }

            // Inside a category: single-indented lines are command summaries;
            // deeper-indented lines are Usage hints — skip them.
            // Accept 1–5 leading spaces so the snapshot is stable across
            // cli-framework help-indent changes (was 2 spaces, now 4 spaces).
            if let Some(ref cat) = current_category {
                let indent = raw_line.len() - raw_line.trim_start().len();
                if (1..=5).contains(&indent) {
                    category_commands
                        .entry(cat.clone())
                        .or_default()
                        .push(trimmed.to_string());
                }
                // Deeper indent (Usage: hints) → skip silently.
            }
        }

        // Reconstruct: Usage + blank + sorted category sections + Options
        if let Some(u) = usage_line {
            out_lines.push(u);
        }
        out_lines.push(String::new());
        for (cat, mut cmds) in category_commands {
            cmds.sort();
            out_lines.push(format!("{}:", cat));
            for cmd in cmds {
                out_lines.push(format!("  {}", cmd));
            }
        }
        options_lines.sort();
        if !options_lines.is_empty() {
            out_lines.push("Options:".to_string());
            for opt in options_lines {
                out_lines.push(format!("  {}", opt));
            }
        }
        out_lines.join("\n")
    }

    let actual = help_output(&[]);
    let expected_raw = include_str!("snapshots/help_parity.snap");
    let expected_body = expected_raw
        .split("\n---\n")
        .nth(1)
        .expect("snapshot has YAML frontmatter terminator")
        .trim_start_matches('\n');

    let actual_norm = normalize(&actual);
    let expected_norm = normalize(expected_body);
    assert_eq!(
        actual_norm.trim(),
        expected_norm.trim(),
        "newton --help drifted from snapshots/help_parity.snap; \
         regenerate the snapshot if the change is intentional.\n\
         Actual normalized:\n{actual_norm}\n\nExpected normalized:\n{expected_norm}"
    );
}

#[test]
fn serve_help_lists_route_groups_and_pointers() {
    let stdout = help_output(&["serve"]);
    for required in [
        "EXAMPLES:",
        "--host",
        "--port",
        "--static-ui",
        "openapi/newton-backend-parity.yaml",
    ] {
        assert!(
            stdout.contains(required),
            "serve --help should contain {:?}, got:\n{}",
            required,
            stdout
        );
    }
}

// ── data subcommand help tests (issue #336) ───────────────────────────────────

#[test]
fn data_help_lists_verb_subcommands() {
    let stdout = help_output(&["data"]);
    for verb in ["get", "post", "put", "patch", "delete"] {
        assert!(
            stdout.contains(verb),
            "data --help should list '{}' subcommand, got:\n{}",
            verb,
            stdout
        );
    }
}

#[test]
fn data_get_help_has_examples_section() {
    let stdout = help_output(&["data", "get"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "data get --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn data_get_help_shows_product_example() {
    let stdout = help_output(&["data", "get"]);
    assert!(
        stdout.contains("newton data get product"),
        "data get --help should show 'newton data get product' example, got:\n{}",
        stdout
    );
}

#[test]
fn data_get_help_does_not_have_dry_run() {
    let stdout = help_output(&["data", "get"]);
    assert!(
        !stdout.contains("--dry-run"),
        "data get --help should NOT list --dry-run, got:\n{}",
        stdout
    );
}

#[test]
fn data_get_help_does_not_have_file_or_body() {
    let stdout = help_output(&["data", "get"]);
    assert!(
        !stdout.contains("--file") && !stdout.contains("-f,"),
        "data get --help should NOT list --file, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("--body"),
        "data get --help should NOT list --body, got:\n{}",
        stdout
    );
}

#[test]
fn data_post_help_has_examples_section() {
    let stdout = help_output(&["data", "post"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "data post --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn data_post_help_shows_file_flag_example() {
    let stdout = help_output(&["data", "post"]);
    assert!(
        stdout.contains("newton data post product") || stdout.contains("-f"),
        "data post --help should show POST example with -f, got:\n{}",
        stdout
    );
}

#[test]
fn data_post_help_has_dry_run() {
    let stdout = help_output(&["data", "post"]);
    assert!(
        stdout.contains("--dry-run"),
        "data post --help should list --dry-run, got:\n{}",
        stdout
    );
}

#[test]
fn data_post_help_does_not_show_get_or_delete_examples() {
    let stdout = help_output(&["data", "post"]);
    assert!(
        !stdout.contains("newton data get products"),
        "data post --help should NOT show GET example, got:\n{}",
        stdout
    );
}

#[test]
fn data_put_help_has_examples_section() {
    let stdout = help_output(&["data", "put"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "data put --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn data_put_help_shows_product_id_example() {
    let stdout = help_output(&["data", "put"]);
    assert!(
        stdout.contains("newton data put product"),
        "data put --help should show 'newton data put product' example, got:\n{}",
        stdout
    );
}

#[test]
fn data_patch_help_has_examples_section() {
    let stdout = help_output(&["data", "patch"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "data patch --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn data_patch_help_shows_product_example() {
    let stdout = help_output(&["data", "patch"]);
    assert!(
        stdout.contains("newton data patch product"),
        "data patch --help should show 'newton data patch product' example, got:\n{}",
        stdout
    );
}

#[test]
fn data_delete_help_has_examples_section() {
    let stdout = help_output(&["data", "delete"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "data delete --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn data_delete_help_shows_product_example() {
    let stdout = help_output(&["data", "delete"]);
    assert!(
        stdout.contains("newton data delete product"),
        "data delete --help should show 'newton data delete product' example, got:\n{}",
        stdout
    );
}

#[test]
fn data_delete_help_does_not_have_dry_run() {
    let stdout = help_output(&["data", "delete"]);
    assert!(
        !stdout.contains("--dry-run"),
        "data delete --help should NOT list --dry-run, got:\n{}",
        stdout
    );
}

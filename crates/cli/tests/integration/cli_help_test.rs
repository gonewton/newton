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
fn run_help_has_examples_section() {
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("EXAMPLES:"),
        "run --help should contain EXAMPLES: section, got:\n{}",
        stdout
    );
}

#[test]
fn run_help_shows_basic_workflow_example() {
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("newton run workflow.yaml"),
        "run --help should show basic usage example"
    );
}

#[test]
fn run_help_shows_workspace_flag_example() {
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("--workspace"),
        "run --help should demonstrate --workspace flag"
    );
}

#[test]
fn run_help_shows_trigger_flag_example() {
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("--trigger"),
        "run --help should demonstrate --trigger flag"
    );
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
        &["run"],
        &["init"],
        &["batch"],
        &["serve"],
        &["workflow"],
        &["resume"],
        &["checkpoint"],
        &["artifact"],
        &["webhook"],
        &["monitor"],
        &["runs"],
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

#[test]
fn run_help_does_not_reference_nonexistent_flags() {
    let stdout = help_output(&["run"]);
    assert!(
        !stdout.contains("--max-iterations"),
        "run --help should not reference removed --max-iterations flag"
    );
    assert!(
        !stdout.contains("--tool-timeout"),
        "run --help should not reference removed --tool-timeout flag"
    );
    assert!(
        !stdout.contains("--strict-mode"),
        "run --help should not reference removed --strict-mode flag"
    );
}

/// Parity check (spec §10 Stage E / §15 D8): asserts that the post-migration
/// `newton --help` output stays in lock-step with the `help_parity.snap`
/// artifact that the CHANGELOG references.  cli-framework prints commands
/// in HashMap order, so we sort both sides before comparing — what we care
/// about is the *set* and rendering shape, not framework iteration order.
#[test]
fn newton_help_matches_parity_snapshot() {
    fn normalize(text: &str) -> String {
        // Bucket lines into "header lines" (Usage / Commands: / Options: /
        // blank) and "indented body lines"; sort the body bucket.
        let mut header_pre: Vec<&str> = Vec::new();
        let mut commands_body: Vec<&str> = Vec::new();
        let mut options_body: Vec<&str> = Vec::new();
        let mut section: &str = "pre";
        for line in text.lines() {
            let trimmed = line.trim_end();
            if trimmed == "Commands:" {
                section = "commands";
                header_pre.push(trimmed);
                continue;
            }
            if trimmed == "Options:" {
                section = "options";
                header_pre.push(trimmed);
                continue;
            }
            match section {
                "pre" => header_pre.push(trimmed),
                "commands" => {
                    if trimmed.is_empty() {
                        section = "between";
                        header_pre.push(trimmed);
                    } else if trimmed.trim_start().starts_with("ask ") {
                        // `ask` is feature-gated; keep the parity snapshot
                        // independent of which feature flags built the bin.
                        continue;
                    } else {
                        commands_body.push(trimmed);
                    }
                }
                "between" => header_pre.push(trimmed),
                "options" => options_body.push(trimmed),
                _ => {}
            }
        }
        commands_body.sort();
        options_body.sort();
        let mut out = String::new();
        let mut emit_commands = false;
        let mut emit_options = false;
        for line in header_pre {
            out.push_str(line);
            out.push('\n');
            if line == "Commands:" {
                for c in &commands_body {
                    out.push_str(c);
                    out.push('\n');
                }
                emit_commands = true;
            }
            if line == "Options:" {
                for c in &options_body {
                    out.push_str(c);
                    out.push('\n');
                }
                emit_options = true;
            }
        }
        // Defensive: if Options: never appeared in pre buffer (shouldn't
        // happen with current renderer), append the bucket at the end.
        if !emit_commands {
            for c in &commands_body {
                out.push_str(c);
                out.push('\n');
            }
        }
        if !emit_options {
            for c in &options_body {
                out.push_str(c);
                out.push('\n');
            }
        }
        out
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
         regenerate the snapshot if the change is intentional"
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

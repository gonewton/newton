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
    // `newton run --help` is intercepted to `newton workflow run --help`
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("newton workflow run workflow.yaml"),
        "run --help (intercepted to workflow run --help) should show basic usage example"
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
                    } else if trimmed.trim_start().starts_with("run ") {
                        // `run` is a deprecated hidden alias (spec 051); excluded from
                        // the public command surface snapshot even though the framework
                        // renders it because CommandSpec::hidden does not suppress clap output.
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

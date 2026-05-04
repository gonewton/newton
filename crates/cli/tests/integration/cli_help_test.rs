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
fn run_help_shows_arg_flag_example() {
    let stdout = help_output(&["run"]);
    assert!(
        stdout.contains("--arg"),
        "run --help should demonstrate --arg flag"
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
        &["validate"],
        &["lint"],
        &["explain"],
        &["resume"],
        &["checkpoints"],
        &["artifacts"],
        &["webhook"],
        &["monitor"],
        &["dot"],
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

#[test]
fn serve_help_omits_endpoint_catalog() {
    let stdout = help_output(&["serve"]);
    for forbidden in [
        "API ENDPOINTS",
        "LEGACY ENDPOINTS",
        "/health",
        "/api/workflows",
        "/api/operators",
        "/api/hil/",
        "/api/stream/",
        "/api/channels",
    ] {
        assert!(
            !stdout.contains(forbidden),
            "serve --help should not contain {:?}, got:\n{}",
            forbidden,
            stdout
        );
    }
    for required in ["EXAMPLES:", "--host", "--port", "--ui-dir"] {
        assert!(
            stdout.contains(required),
            "serve --help should contain {:?}, got:\n{}",
            required,
            stdout
        );
    }
}

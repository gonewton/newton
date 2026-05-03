use std::process::Command;

// Command lists for testing
static ALL_MAIN_COMMANDS: &[&[&str]] = &[
    &["run"],
    &["init"],
    &["batch"],
    &["monitor"],
    &["validate"],
    &["dot"],
    &["lint"],
    &["explain"],
    &["resume"],
    &["checkpoints"],
    &["artifacts"],
    &["webhook"],
];

static COMPLEX_COMMANDS: &[&[&str]] = &[
    &["run"],
    &["monitor"],
    &["validate"],
    &["dot"],
    &["lint"],
    &["explain"],
    &["resume"],
    &["checkpoints"],
    &["artifacts"],
    &["webhook"],
];

/// Generic helper to run tests on multiple commands
fn test_commands_with<F>(commands: &[&[&str]], test_fn: F)
where
    F: Fn(&[&str]),
{
    for command in commands {
        test_fn(command);
    }
}

/// Helper function to run a command and get help output
fn get_help_output(args: &[&str]) -> String {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(args)
        .arg("--help")
        .output()
        .expect("should run successfully");

    std::str::from_utf8(&output.stdout).unwrap().to_string()
}

/// Test that help text is descriptive and user-friendly (not just technical terms)
fn assert_help_is_descriptive(command_args: &[&str], min_length: usize) {
    let stdout = get_help_output(command_args);

    // Should contain descriptive language
    let descriptive_indicators = [
        "help",
        "allows",
        "provides",
        "creates",
        "generates",
        "manages",
        "enables",
        "analyzes",
        "checks",
        "displays",
        "useful",
        "when",
    ];

    let has_descriptive_language = descriptive_indicators
        .iter()
        .any(|&indicator| stdout.to_lowercase().contains(indicator));

    assert!(
        has_descriptive_language,
        "Help text for {:?} should contain descriptive language",
        command_args
    );

    // Help should be reasonably long (more descriptive than minimal)
    assert!(
        stdout.len() > min_length,
        "Help text for {:?} should be at least {} characters, got {}",
        command_args,
        min_length,
        stdout.len()
    );
}

/// Test that complex commands include examples
fn assert_has_examples(command_args: &[&str]) {
    let stdout = get_help_output(command_args);
    assert!(
        stdout.contains("EXAMPLES") || stdout.contains("Example"),
        "Help text for {:?} should contain examples section",
        command_args
    );
}

/// Test that help follows consistent formatting standards
fn assert_consistent_formatting(command_args: &[&str]) {
    let stdout = get_help_output(command_args);

    // Should have consistent section headers (UPPERCASE)
    let has_proper_sections =
        stdout.contains("EXAMPLES") || stdout.contains("USAGE") || stdout.contains("OPTIONS");

    assert!(
        has_proper_sections,
        "Help text for {:?} should have properly formatted section headers",
        command_args
    );
}

#[test]
fn test_monitor_help_contains_configuration_section() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("CONFIGURATION"));
}

#[test]
fn test_monitor_help_contains_examples_section() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("EXAMPLES"));
}

#[test]
fn test_monitor_help_contains_troubleshooting_section() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("TROUBLESHOOTING"));
}

#[test]
fn test_monitor_help_describes_endpoint_pairing() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("HTTP and WebSocket endpoints"));
    assert!(stdout.contains("--http-url"));
    assert!(stdout.contains("--ws-url"));
}

#[test]
fn test_monitor_help_includes_cli_example() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("newton monitor --http-url"));
    assert!(stdout.contains("--ws-url"));
}

#[test]
fn test_monitor_help_includes_config_file_example() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("monitor.conf"));
    assert!(stdout.contains("ailoop_server_http_url"));
    assert!(stdout.contains("ailoop_server_ws_url"));
}

#[test]
fn test_monitor_help_shows_discovery_order() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("Endpoint discovery order"));
    assert!(stdout.contains("CLI overrides"));
    assert!(stdout.contains("monitor.conf"));
}

#[test]
fn test_validate_help_documents_positional_and_file_workflow_forms() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(["validate", "--help"])
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("newton validate workflow.yaml"));
    assert!(stdout.contains("[WORKFLOW]"));
    assert!(stdout.contains("--file <PATH>"));
}

#[test]
fn test_run_help_keeps_two_positional_arguments_in_order() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(["run", "--help"])
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("[WORKFLOW] [INPUT_FILE]"));
    assert!(stdout.contains("--file <PATH>"));
}

#[test]
fn test_webhook_help_documents_positional_workflow_example() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(["webhook", "--help"])
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains("newton webhook serve workflow.yaml --workspace ./workspace"));
}

// Tests for improved help text descriptiveness and quality

#[test]
fn test_all_main_commands_have_descriptive_help() {
    test_commands_with(ALL_MAIN_COMMANDS, |command| {
        assert_help_is_descriptive(command, 200); // At least 200 chars of help
    });
}

#[test]
fn test_complex_commands_include_examples() {
    test_commands_with(COMPLEX_COMMANDS, |command| {
        assert_has_examples(command);
    });
}

#[test]
fn test_help_formatting_consistency() {
    test_commands_with(ALL_MAIN_COMMANDS, |command| {
        assert_consistent_formatting(command);
    });
}

// Specific tests for improved commands

#[test]
fn test_validate_help_explains_file_checking() {
    let stdout = get_help_output(&["validate"]);
    assert!(stdout.contains("checks your workflow YAML file"));
}

#[test]
fn test_validate_help_mentions_syntax_errors() {
    let stdout = get_help_output(&["validate"]);
    assert!(stdout.contains("syntax errors"));
}

#[test]
fn test_validate_help_explains_pre_execution_purpose() {
    let stdout = get_help_output(&["validate"]);
    assert!(stdout.contains("before execution"));
}

#[test]
fn test_validate_help_documents_return_codes() {
    let stdout = get_help_output(&["validate"]);
    assert!(stdout.contains("RETURN CODES"));
}

#[test]
fn test_dot_help_mentions_visual_diagram() {
    let stdout = get_help_output(&["dot"]);
    assert!(stdout.contains("visual diagram"));
}

#[test]
fn test_dot_help_references_graphviz() {
    let stdout = get_help_output(&["dot"]);
    assert!(stdout.contains("Graphviz"));
}

#[test]
fn test_dot_help_explains_task_dependencies() {
    let stdout = get_help_output(&["dot"]);
    assert!(stdout.contains("task dependencies"));
}

#[test]
fn test_dot_help_has_visualization_section() {
    let stdout = get_help_output(&["dot"]);
    assert!(stdout.contains("VISUALIZATION"));
}

#[test]
fn test_lint_help_mentions_best_practices() {
    let stdout = get_help_output(&["lint"]);
    assert!(stdout.contains("best practices"));
}

#[test]
fn test_lint_help_distinguishes_from_validate() {
    let stdout = get_help_output(&["lint"]);
    assert!(stdout.contains("Unlike validate"));
}

#[test]
fn test_lint_help_emphasizes_quality() {
    let stdout = get_help_output(&["lint"]);
    assert!(stdout.contains("quality"));
}

#[test]
fn test_lint_help_documents_output_formats() {
    let stdout = get_help_output(&["lint"]);
    assert!(stdout.contains("OUTPUT FORMATS"));
}

#[test]
fn test_explain_help_mentions_detailed_documentation() {
    let stdout = get_help_output(&["explain"]);
    assert!(stdout.contains("detailed documentation"));
}

#[test]
fn test_explain_help_describes_step_by_step() {
    let stdout = get_help_output(&["explain"]);
    assert!(stdout.contains("Step-by-step"));
}

#[test]
fn test_explain_help_documents_output_formats() {
    let stdout = get_help_output(&["explain"]);
    assert!(stdout.contains("OUTPUT FORMATS"));
}

#[test]
fn test_explain_help_mentions_prose_format() {
    let stdout = get_help_output(&["explain"]);
    assert!(stdout.contains("prose"));
}

#[test]
fn test_resume_help_mentions_interrupted_workflows() {
    let stdout = get_help_output(&["resume"]);
    assert!(stdout.contains("interrupted"));
}

#[test]
fn test_resume_help_explains_checkpoint_concept() {
    let stdout = get_help_output(&["resume"]);
    assert!(stdout.contains("checkpoint"));
}

#[test]
fn test_resume_help_has_finding_execution_section() {
    let stdout = get_help_output(&["resume"]);
    assert!(stdout.contains("FINDING EXECUTION"));
}

#[test]
fn test_resume_help_documents_safety_considerations() {
    let stdout = get_help_output(&["resume"]);
    assert!(stdout.contains("SAFETY"));
}

#[test]
fn test_resume_help_documents_workflow_change_option() {
    let stdout = get_help_output(&["resume"]);
    assert!(stdout.contains("--allow-workflow-change"));
}

#[test]
fn test_checkpoints_help_explains_saved_states() {
    let stdout = get_help_output(&["checkpoints"]);
    assert!(stdout.contains("saved states"));
}

#[test]
fn test_checkpoints_help_mentions_resumption() {
    let stdout = get_help_output(&["checkpoints"]);
    assert!(stdout.contains("resumption"));
}

#[test]
fn test_checkpoints_help_explains_automatic_creation() {
    let stdout = get_help_output(&["checkpoints"]);
    assert!(stdout.contains("automatically creates"));
}

#[test]
fn test_checkpoints_help_documents_storage_section() {
    let stdout = get_help_output(&["checkpoints"]);
    assert!(stdout.contains("CHECKPOINT STORAGE"));
}

#[test]
fn test_artifacts_help_mentions_output_files() {
    let stdout = get_help_output(&["artifacts"]);
    assert!(stdout.contains("output files"));
}

#[test]
fn test_artifacts_help_explains_retention_concept() {
    let stdout = get_help_output(&["artifacts"]);
    assert!(stdout.contains("retention"));
}

#[test]
fn test_artifacts_help_mentions_disk_space() {
    let stdout = get_help_output(&["artifacts"]);
    assert!(stdout.contains("disk space"));
}

#[test]
fn test_artifacts_help_documents_retention_formats() {
    let stdout = get_help_output(&["artifacts"]);
    assert!(stdout.contains("RETENTION FORMATS"));
}

#[test]
fn test_artifacts_help_documents_storage_section() {
    let stdout = get_help_output(&["artifacts"]);
    assert!(stdout.contains("ARTIFACT STORAGE"));
}

#[test]
fn test_webhook_help_mentions_external_events() {
    let stdout = get_help_output(&["webhook"]);
    assert!(stdout.contains("external events"));
}

#[test]
fn test_webhook_help_explains_integration_concept() {
    let stdout = get_help_output(&["webhook"]);
    assert!(stdout.contains("integration"));
}

#[test]
fn test_webhook_help_mentions_github_integration() {
    let stdout = get_help_output(&["webhook"]);
    assert!(stdout.contains("GitHub"));
}

#[test]
fn test_webhook_help_has_integration_section() {
    let stdout = get_help_output(&["webhook"]);
    assert!(stdout.contains("INTEGRATION"));
}

#[test]
fn test_webhook_help_documents_security_considerations() {
    let stdout = get_help_output(&["webhook"]);
    assert!(stdout.contains("SECURITY"));
}

// Test subcommand help improvements

#[test]
fn test_webhook_serve_help_is_descriptive() {
    let stdout = get_help_output(&["webhook", "serve"]);
    assert!(stdout.contains("Start an HTTP server"));
    assert!(stdout.contains("webhook events"));
}

#[test]
fn test_webhook_status_help_is_descriptive() {
    let stdout = get_help_output(&["webhook", "status"]);
    assert!(stdout.contains("Display webhook endpoint"));
    assert!(stdout.contains("configuration"));
}

#[test]
fn test_checkpoints_list_help_is_descriptive() {
    let stdout = get_help_output(&["checkpoints", "list"]);
    assert!(stdout.contains("Display available"));
    assert!(stdout.contains("checkpoint details"));
}

#[test]
fn test_checkpoints_clean_help_is_descriptive() {
    let stdout = get_help_output(&["checkpoints", "clean"]);
    assert!(stdout.contains("Remove old checkpoint"));
    assert!(stdout.contains("disk space"));
}

#[test]
fn test_artifacts_clean_help_is_descriptive() {
    let stdout = get_help_output(&["artifacts", "clean"]);
    assert!(stdout.contains("Remove old workflow"));
    assert!(stdout.contains("execution artifacts"));
}

#[test]
fn test_log_list_help_has_examples() {
    let stdout = get_help_output(&["log", "list"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton log list"));
}

#[test]
fn test_log_show_help_has_examples() {
    let stdout = get_help_output(&["log", "show"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton log show"));
}

#[test]
fn test_checkpoints_list_help_has_examples() {
    let stdout = get_help_output(&["checkpoints", "list"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton checkpoints list"));
}

#[test]
fn test_checkpoints_clean_help_has_examples() {
    let stdout = get_help_output(&["checkpoints", "clean"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton checkpoints clean"));
    assert!(stdout.contains("--older-than"));
}

#[test]
fn test_artifacts_clean_help_has_examples() {
    let stdout = get_help_output(&["artifacts", "clean"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton artifacts clean"));
}

#[test]
fn test_webhook_serve_help_has_examples() {
    let stdout = get_help_output(&["webhook", "serve"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton webhook serve"));
}

#[test]
fn test_webhook_status_help_has_examples() {
    let stdout = get_help_output(&["webhook", "status"]);
    assert!(stdout.contains("EXAMPLES"));
    assert!(stdout.contains("newton webhook status"));
}

// Test that help text avoids overly technical jargon

#[test]
fn test_explain_help_avoids_technical_jargon() {
    let stdout = get_help_output(&["explain"]);

    // Should NOT contain the old technical phrase
    assert!(!stdout.contains("Explain workflow graph settings/transitions"));

    // Should contain user-friendly language instead
    assert!(stdout.contains("detailed documentation"));
    assert!(stdout.contains("what your workflow does"));
}

#[test]
fn test_help_text_quality_meets_spec_requirements() {
    // Test that improved help text is at least 2x longer than minimal descriptions
    let commands_to_check = [
        (&["validate"][..], "Validate checks your workflow"),
        (&["dot"][..], "visual diagram"),
        (&["lint"][..], "best practices"),
        (&["explain"][..], "detailed documentation"),
        (&["resume"][..], "interrupted"),
    ];

    for (command, expected_content) in &commands_to_check {
        let stdout = get_help_output(command);
        assert!(
            stdout.contains(expected_content),
            "Help for {:?} should contain: {}",
            command,
            expected_content
        );

        // Verify help is substantially longer than minimal descriptions
        assert!(
            stdout.len() > 500,
            "Help for {:?} should be substantially detailed (>500 chars), got {}",
            command,
            stdout.len()
        );
    }
}

//! High-level help-text smoke tests under the spec 273 surface.
//!
//! Verifies that each of the new top-level commands renders descriptive
//! help and contains an examples block.

use std::process::Command;

static ALL_MAIN_COMMANDS: &[&[&str]] = &[&["init"], &["optimize"], &["workflow"], &["serve"]];

fn get_help_output(args: &[&str]) -> String {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(args)
        .arg("--help")
        .output()
        .expect("should run successfully");
    std::str::from_utf8(&output.stdout).unwrap().to_string()
}

#[test]
fn test_all_main_commands_render_help() {
    for command in ALL_MAIN_COMMANDS {
        let stdout = get_help_output(command);
        assert!(
            !stdout.is_empty(),
            "Help text for {:?} should be non-empty",
            command
        );
        assert!(
            stdout.len() > 80,
            "Help text for {:?} should be at least 80 chars, got {}",
            command,
            stdout.len()
        );
    }
}

#[test]
fn test_all_main_commands_have_examples_section() {
    for command in ALL_MAIN_COMMANDS {
        let stdout = get_help_output(command);
        assert!(
            stdout.contains("EXAMPLES") || stdout.contains("Example"),
            "Help text for {:?} should contain examples section",
            command
        );
    }
}

#[test]
fn workflow_subcommand_help_works() {
    for sub in ["validate", "lint", "preview", "graph"] {
        let stdout = get_help_output(&["workflow", sub]);
        assert!(
            !stdout.is_empty(),
            "newton workflow {} --help should produce non-empty output",
            sub
        );
    }
}

#[test]
fn resume_help_documents_run_id_flag() {
    let stdout = get_help_output(&["workflow"]);
    assert!(stdout.contains("--run-id"));
    assert!(!stdout.contains("--execution-id"));
}

#[test]
fn optimize_help_documents_poll_interval_not_sleep() {
    let stdout = get_help_output(&["optimize"]);
    assert!(stdout.contains("--poll-interval"));
    assert!(!stdout.contains("--sleep "));
}

#[test]
fn init_help_documents_template_flag() {
    let stdout = get_help_output(&["init"]);
    assert!(stdout.contains("--template"));
    assert!(!stdout.contains("--template-source"));
}

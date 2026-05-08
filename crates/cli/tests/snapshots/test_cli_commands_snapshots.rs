use insta::assert_snapshot;
use std::process::Command;

#[test]
fn version_flag_snapshot() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("--version")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();

    // Replace version number with placeholder to avoid snapshot invalidation on version changes
    let normalized = regex::Regex::new(r"\d+\.\d+\.\d+")
        .unwrap()
        .replace_all(stdout, "[VERSION]");

    assert_snapshot!("version_output", normalized);
}

#[test]
fn help_flag_snapshot() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("--help")
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();

    // Replace version number with placeholder to avoid snapshot invalidation on version changes
    let normalized = regex::Regex::new(r"\d+\.\d+\.\d+")
        .unwrap()
        .replace_all(stdout, "[VERSION]");

    assert_snapshot!("help_output", normalized);
}

#[test]
fn run_command_help_snapshot() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(["run", "--help"])
        .output()
        .expect("should run successfully");

    assert_snapshot!(
        "run_help_output",
        std::str::from_utf8(&output.stdout).unwrap()
    );
}

#[test]
fn init_command_help_snapshot() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .args(["init", "--help"])
        .output()
        .expect("should run successfully");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let normalized = regex::Regex::new(r"\d+\.\d+\.\d+")
        .unwrap()
        .replace_all(stdout, "[VERSION]");

    assert_snapshot!("init_help_output", normalized);
}

// --- §7 criterion 8 / Stage 2: shell-completion fixtures (bash/zsh/fish/powershell) ---
//
// We assert that none of the legacy 273 spellings reappear in the
// completion output for any of the four supported shells.

fn assert_no_legacy_spellings(output: &str, shell: &str) {
    for legacy in [
        "--trigger-json",
        "--max-time-seconds",
        "--execution-id",
        "--ui-dir",
        "--http-url",
        "--ws-url",
        "--template-source",
        "--format-json",
    ] {
        assert!(
            !output.contains(legacy),
            "completion {shell} still mentions legacy flag {legacy}"
        );
    }
}

#[test]
fn completion_outputs_have_no_legacy_spellings() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
            .args(["completion", shell])
            .output()
            .unwrap_or_else(|e| panic!("ran completion {shell}: {e}"));
        assert!(output.status.success(), "completion {shell} failed");
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert_no_legacy_spellings(stdout, shell);
    }
}

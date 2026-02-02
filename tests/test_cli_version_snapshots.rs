use assert_cmd::Command;
use insta::assert_snapshot;

const BIN: &str = "newton";

#[test]
fn version_flag_snapshot() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("--version")
        .output()
        .expect("should run successfully");

    assert_snapshot!(
        "version_output",
        std::str::from_utf8(&output.stdout).unwrap()
    );
}

#[test]
fn help_flag_snapshot() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("--help")
        .output()
        .expect("should run successfully");

    assert_snapshot!("help_output", std::str::from_utf8(&output.stdout).unwrap());
}

#[test]
fn run_command_help_snapshot() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .args(&["run", "--help"])
        .output()
        .expect("should run successfully");

    assert_snapshot!(
        "run_help_output",
        std::str::from_utf8(&output.stdout).unwrap()
    );
}

#[test]
fn step_command_help_snapshot() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .args(&["step", "--help"])
        .output()
        .expect("should run successfully");

    assert_snapshot!(
        "step_help_output",
        std::str::from_utf8(&output.stdout).unwrap()
    );
}

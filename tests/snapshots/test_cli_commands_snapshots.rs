use insta::assert_snapshot;
use std::process::Command;

#[test]
fn version_flag_snapshot() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
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
    let output = Command::new(assert_cmd::cargo::cargo_bin!("newton"))
        .arg("--help")
        .output()
        .expect("should run successfully");

    assert_snapshot!("help_output", std::str::from_utf8(&output.stdout).unwrap());
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

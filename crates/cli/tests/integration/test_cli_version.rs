use assert_cmd::Command;
use predicates::str::starts_with;

const BIN: &str = "newton";

#[test]
fn version_flag_prints_crate_version() {
    let expected = format!("{BIN} {}", newton_cli::VERSION);

    Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("--version")
        .assert()
        .success()
        .stdout(starts_with(expected));
}

#[test]
fn health_command_prints_ok_with_version() {
    let expected = format!("newton OK {}", newton_cli::VERSION);
    Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("health")
        .assert()
        .success()
        .stdout(starts_with(expected));
}

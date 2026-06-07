use assert_cmd::Command;

const BIN: &str = "newton";

#[test]
fn mcp_install_dry_run_exits_zero_and_prints_config() {
    let mut cmd = Command::cargo_bin(BIN).expect("binary should build");
    let output = cmd
        .args([
            "mcp",
            "install",
            "--agent",
            "cursor",
            "--stdio",
            "--dry-run",
        ])
        .output()
        .expect("should run");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mcp") && stdout.contains("serve") && stdout.contains("stdio"),
        "stdout should reference mcp serve --transport stdio, got:\n{}",
        stdout
    );
}

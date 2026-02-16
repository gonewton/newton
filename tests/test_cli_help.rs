use std::process::Command;

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

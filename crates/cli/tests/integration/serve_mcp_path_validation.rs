//! Issue #294: `serve --with-mcp` MUST reject invalid `--mcp-path` values
//! (`NEWTON-SERVE-MCP-001`) and values that collide with an existing Newton
//! REST route prefix (`NEWTON-SERVE-MCP-002`) before binding.
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn rejects_path_without_leading_slash() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--with-mcp",
            "--mcp-path",
            "foo",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-MCP-001"));
}

#[test]
fn rejects_path_colliding_with_health() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--with-mcp",
            "--mcp-path",
            "/health",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-MCP-002"));
}

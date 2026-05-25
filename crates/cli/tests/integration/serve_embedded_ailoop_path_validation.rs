//! Issue #351: `serve --with-embedded-ailoop` MUST reject invalid
//! `--ailoop-base-path` values (`NEWTON-SERVE-AIL-001`), values that collide
//! with Newton REST route prefixes (`NEWTON-SERVE-AIL-002`), and values that
//! collide with `--mcp-path` when both flags are active (`NEWTON-SERVE-AIL-003`).
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

// ── NEWTON-SERVE-AIL-001: invalid path shape ──────────────────────────────────

#[test]
fn rejects_empty_base_path() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args(["serve", "--with-embedded-ailoop", "--ailoop-base-path", ""])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-001"));
}

#[test]
fn rejects_base_path_without_leading_slash() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "ailoop",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-001"));
}

#[test]
fn rejects_bare_root() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args(["serve", "--with-embedded-ailoop", "--ailoop-base-path", "/"])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-001"));
}

#[test]
fn rejects_trailing_slash() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop/",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-001"));
}

#[test]
fn rejects_api_base_path() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/api",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-002"));
}

// ── NEWTON-SERVE-AIL-002: Newton REST route prefix collision ──────────────────

#[test]
fn rejects_collision_with_health() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/health",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-002"));
}

#[test]
fn rejects_collision_with_workflows() {
    let dir = tempdir().expect("tempdir");
    Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/workflows",
        ])
        .assert()
        .failure()
        .stderr(contains("NEWTON-SERVE-AIL-002"));
}

#[test]
fn allows_non_colliding_paths() {
    let dir = tempdir().expect("tempdir");
    // This should NOT fail on path validation.
    let output = Command::cargo_bin("newton")
        .expect("binary builds")
        .current_dir(dir.path())
        .args([
            "serve",
            "--with-mcp",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop",
            "--port",
            "0",
        ])
        .timeout(std::time::Duration::from_secs(5))
        .output()
        .expect("command ran");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("NEWTON-SERVE-AIL-001"),
        "unexpected NEWTON-SERVE-AIL-001 in stderr: {stderr}"
    );
    assert!(
        !stderr.contains("NEWTON-SERVE-AIL-002"),
        "unexpected NEWTON-SERVE-AIL-002 in stderr: {stderr}"
    );
}

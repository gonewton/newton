//! spec 074, B20: `doctor` must resolve the workspace exactly once (explicit
//! `--workspace`, else CWD-if-`.newton/`-exists) and pass that single
//! resolution to every probe. Previously only the `workspace` probe applied
//! the CWD fallback while the `config`/`ailoop` probes read `args.workspace`
//! directly, so running `newton doctor` from inside a valid workspace with no
//! `--workspace` flag always reported `SKIP config` / `SKIP ailoop` even
//! though `monitor.conf` existed right there.
#[path = "../support/mod.rs"]
mod support;

use support::newton;

fn write_monitor_conf(workspace: &std::path::Path) {
    std::fs::create_dir_all(workspace.join(".newton/configs")).unwrap();
    std::fs::write(
        workspace.join(".newton/configs/monitor.conf"),
        "ailoop_server_http_url = http://127.0.0.1:1\n",
    )
    .unwrap();
}

#[test]
fn doctor_cwd_fallback_checks_config_and_ailoop_probes() {
    let dir = tempfile::tempdir().unwrap();
    write_monitor_conf(dir.path());

    // No --workspace flag: doctor must fall back to CWD (set on the child
    // process via `current_dir`, not this test process).
    let out = newton()
        .current_dir(dir.path())
        .args(["doctor"])
        .output()
        .expect("newton doctor should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("SKIP config"),
        "config probe should be checked via the CWD fallback, not SKIPped; got: {stdout}"
    );
    assert!(
        !stdout.contains("SKIP ailoop"),
        "ailoop probe should be checked (OK or FAIL — monitor.conf declares \
         ailoop_server_http_url) via the CWD fallback, not SKIPped; got: {stdout}"
    );
    assert!(
        stdout.contains("OK config"),
        "config probe should report OK once monitor.conf is found via the CWD fallback; \
         got: {stdout}"
    );
}

#[test]
fn doctor_explicit_workspace_still_checks_config_probe() {
    // Regression guard: the explicit --workspace path must keep behaving
    // exactly as it did before the CWD-resolution fix.
    let dir = tempfile::tempdir().unwrap();
    write_monitor_conf(dir.path());

    let out = newton()
        .args(["doctor", "--workspace", &dir.path().to_string_lossy()])
        .output()
        .expect("newton doctor should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK config"),
        "config probe should report OK with an explicit --workspace; got: {stdout}"
    );
}

#[test]
fn doctor_no_workspace_anywhere_skips_config_probe() {
    // No .newton/ in CWD and no --workspace: config/ailoop must legitimately
    // SKIP (nothing changed about this case).
    let dir = tempfile::tempdir().unwrap();

    let out = newton()
        .current_dir(dir.path())
        .args(["doctor"])
        .output()
        .expect("newton doctor should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("SKIP config"),
        "config probe should SKIP when there is no workspace at all; got: {stdout}"
    );
    assert!(
        stdout.contains("SKIP ailoop"),
        "ailoop probe should SKIP when there is no workspace at all; got: {stdout}"
    );
}

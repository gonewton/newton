#[path = "../support/mod.rs"]
mod support;

use std::fs;
use support::newton;

#[test]
fn integ_init_creates_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("my-project");
    fs::create_dir_all(&project).unwrap();

    let out = newton()
        .args(["init", &project.to_string_lossy()])
        .output()
        .expect("newton init should execute");

    let newton_dir = project.join(".newton");
    assert!(
        newton_dir.exists(),
        "init should create .newton/ directory; status={:?}, stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        newton_dir.join("configs").exists(),
        "init should create .newton/configs/"
    );
}

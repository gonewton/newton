#[path = "../support/mod.rs"]
mod support;

use support::{newton, TempWorkspace};

#[test]
fn integ_artifact_clean_removes_old() {
    let ws = TempWorkspace::new();
    let artifact = ws.write_artifact("run1", "output.txt", b"test artifact data");
    assert!(
        artifact.exists(),
        "artifact file should exist after writing"
    );

    std::process::Command::new("touch")
        .args(["-t", "202001010000", &artifact.to_string_lossy()])
        .output()
        .expect("touch to set mtime");

    newton()
        .args([
            "artifact",
            "clean",
            "--workspace",
            &ws.path().to_string_lossy(),
            "--older-than",
            "1s",
        ])
        .assert()
        .success();

    assert!(
        !artifact.exists(),
        "old artifact file should be removed after clean"
    );
}

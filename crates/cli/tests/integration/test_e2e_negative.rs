#[path = "../support/mod.rs"]
mod support;

use support::{newton, TempWorkspace};

#[test]
fn negative_checkpoint_clean_missing_older_than() {
    let ws = TempWorkspace::new();
    let out = newton()
        .args([
            "checkpoint",
            "clean",
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();

    assert!(
        !out.status.success()
            || combined.contains("older-than")
            || combined.contains("required")
            || combined.contains("error"),
        "checkpoint clean without --older-than should fail; got: {combined}"
    );
}

#[test]
fn negative_artifact_clean_missing_older_than() {
    let ws = TempWorkspace::new();
    let out = newton()
        .args([
            "artifact",
            "clean",
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();

    assert!(
        !out.status.success()
            || combined.contains("older-than")
            || combined.contains("required")
            || combined.contains("error"),
        "artifact clean without --older-than should fail; got: {combined}"
    );
}

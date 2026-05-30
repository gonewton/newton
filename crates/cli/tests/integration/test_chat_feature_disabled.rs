use serial_test::serial;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` points at `crates/cli` for this crate.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("expected workspace root at ../../ from crates/cli")
        .to_path_buf()
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
}

#[test]
#[serial(chat_feature_disabled)]
fn chat_feature_disabled_emits_error_code() {
    // Build a `newton` binary with `--no-default-features` to ensure
    // `cli-framework/chat` is not enabled (see crates/cli/Cargo.toml).
    let target_dir = tempfile::tempdir().expect("temp target dir");
    let root = workspace_root();

    let status = Command::new("cargo")
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", target_dir.path())
        .args(["build", "-p", "newton-cli", "--no-default-features", "--bin", "newton"])
        .status()
        .expect("cargo build should run");
    assert!(status.success(), "cargo build --no-default-features failed");

    let exe = target_dir
        .path()
        .join("debug")
        .join(format!("newton{}", std::env::consts::EXE_SUFFIX));

    // Calling `newton chat` must fail deterministically with the framework's
    // `CHAT_FEATURE_DISABLED` error code in chat-disabled builds.
    let out = Command::new(&exe)
        .args(["chat", "-p", "hello"])
        .env_remove("OPENAI_API_KEY")
        .env_remove("AIKIT_API_KEY")
        .env_remove("AIKIT_LLM_URL")
        .env_remove("AIKIT_MODEL")
        .output()
        .expect("run newton chat");

    assert!(
        !out.status.success(),
        "expected chat-disabled build to fail; status={:?}",
        out.status
    );
    let text = combined_output(&out.stdout, &out.stderr);
    assert!(
        text.contains("CHAT_FEATURE_DISABLED"),
        "expected CHAT_FEATURE_DISABLED in output, got:\n{text}"
    );
}

#[test]
#[ignore]
fn ext_ask_with_wiremock() {
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let out = std::process::Command::new(bin)
        .args(["ask", "how do I run a workflow"])
        .output()
        .expect("newton ask should execute");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        out.status.success() || combined.contains("CLI-ASK"),
        "ask should succeed or return a CLI-ASK error; stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

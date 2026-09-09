#[path = "support/mod.rs"]
mod support;

use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn newton() -> assert_cmd::Command {
    let mut command = support::newton();
    command
        .arg("--log-dir")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-logs/dependency"));
    command
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/dependency-planning")
        .join(name)
}

fn inspect(map: &Path) -> Value {
    let output = newton()
        .args(["dependency", "inspect", "--map"])
        .arg(map)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("inspection must emit only JSON")
}

fn human_review(directory: &Path, review: &Value, acknowledge: bool) -> PathBuf {
    let path = directory.join("human-review.json");
    let issues: Vec<_> = if acknowledge {
        review["map"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["id"].clone())
            .collect()
    } else {
        vec![]
    };
    fs::write(&path, serde_json::to_vec(&json!({
        "map_fingerprint": review["map_fingerprint"],
        "reviewed_by": "authorized-human-test-fixture",
        "reviewed_at": "2026-09-09T00:00:00Z",
        "completeness_statement": "Reviewed package and cross-service relationships for the fixture product",
        "acknowledged_issues": issues,
    })).unwrap()).unwrap();
    path
}

fn approve(directory: &Path, map: &Path) -> PathBuf {
    let review = inspect(map);
    let human = human_review(directory, &review, true);
    let baseline = directory.join("baseline.json");
    newton()
        .args(["dependency", "approve", "--map"])
        .arg(map)
        .arg("--review")
        .arg(human)
        .arg("--output")
        .arg(&baseline)
        .assert()
        .success();
    baseline
}

fn impact(baseline: &Path, changes: Option<&Path>) -> Vec<u8> {
    let mut command = newton();
    command
        .args(["dependency", "impact", "--baseline"])
        .arg(baseline)
        .args(["--changed", "base", "--target", "product"]);
    if let Some(changes) = changes {
        command.arg("--changes").arg(changes);
    }
    command.assert().success().get_output().stdout.clone()
}

#[test]
fn dependency_help_has_examples_and_read_only_planner_contract() {
    for verb in ["discover", "inspect", "approve", "impact"] {
        let output = newton()
            .args(["dependency", verb, "--help"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("EXAMPLES:"), "{verb}: {text}");
        assert!(text.contains(&format!("newton dependency {verb}")));
    }
}

#[test]
fn dependency_cli_approved_baseline_scopes_impact_without_inventing_versions() {
    let directory = TempDir::new().unwrap();
    let baseline = approve(directory.path(), &example("map.json"));
    let before = fs::read(&baseline).unwrap();
    let first = impact(&baseline, None);
    assert_eq!(
        first,
        impact(&baseline, None),
        "JSON output must be byte-deterministic"
    );
    let sequence: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(sequence["reaches_target"], true);
    let modules: Vec<_> = sequence["stages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|stage| stage["groups"].as_array().unwrap())
        .flat_map(|group| group["members"].as_array().unwrap())
        .collect();
    assert_eq!(
        modules
            .iter()
            .map(|member| member["artifact"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["base", "middle", "app"]
    );
    assert!(modules.iter().all(|member| member["effort"] == "unknown"));
    assert!(modules
        .iter()
        .all(|member| member["artifact"]["version"]["value"] == "1.0.0"));
    let compatible: Value =
        serde_json::from_slice(&impact(&baseline, Some(&example("changes.json")))).unwrap();
    assert!(compatible["unknown_compatibility"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        compatible["stages"].as_array().unwrap().len(),
        3,
        "compatible hops must not be pruned"
    );
    assert_eq!(
        fs::read(&baseline).unwrap(),
        before,
        "query must not assign/persist versions"
    );
}

#[test]
fn dependency_cli_rejects_missing_tampered_or_overwritten_approval() {
    let directory = TempDir::new().unwrap();
    newton()
        .args(["dependency", "impact", "--baseline"])
        .arg(example("map.json"))
        .args(["--changed", "base", "--target", "product"])
        .assert()
        .failure();
    let baseline = approve(directory.path(), &example("map.json"));
    let original = fs::read(&baseline).unwrap();
    let review = human_review(directory.path(), &inspect(&example("map.json")), true);
    newton()
        .args(["dependency", "approve", "--map"])
        .arg(example("map.json"))
        .arg("--review")
        .arg(&review)
        .arg("--output")
        .arg(&baseline)
        .assert()
        .failure();
    assert_eq!(fs::read(&baseline).unwrap(), original);
    let mut altered: Value = serde_json::from_slice(&original).unwrap();
    altered["map"]["dependencies"].as_array_mut().unwrap().pop();
    fs::write(&baseline, serde_json::to_vec(&altered).unwrap()).unwrap();
    newton()
        .args(["dependency", "impact", "--baseline"])
        .arg(&baseline)
        .args(["--changed", "base", "--target", "product"])
        .assert()
        .failure();
    // The old human review cannot approve modified facts either.
    newton()
        .args(["dependency", "approve", "--map"])
        .arg(&baseline)
        .arg("--review")
        .arg(&review)
        .arg("--output")
        .arg(directory.path().join("replacement.json"))
        .assert()
        .failure();
}

#[test]
fn dependency_cli_real_manifest_rediscovery_preserves_declarations_and_is_idempotent() {
    let directory = TempDir::new().unwrap();
    let mut command = newton();
    command
        .args(["dependency", "discover", "--map"])
        .arg(example("map.json"))
        .arg("--manifest")
        .arg(example("Cargo.toml"))
        .arg("--catalog")
        .arg(example("catalog.json"))
        .args(["--owner", "app"]);
    let output = command.assert().success().get_output().stdout.clone();
    let first: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(first["approval_required"], true);
    assert_eq!(
        first["discovery_report"]["dependencies"][0]["discovery"]["kind"],
        "detected"
    );
    let declared = first["map"]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["discovery"]["kind"] == "declared")
        .count();
    assert_eq!(declared, 3);
    let draft = directory.path().join("rediscovered.json");
    fs::write(&draft, &output).unwrap();
    let repeated = newton()
        .args(["dependency", "discover", "--map"])
        .arg(&draft)
        .arg("--manifest")
        .arg(example("Cargo.toml"))
        .arg("--catalog")
        .arg(example("catalog.json"))
        .args(["--owner", "app"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(output, repeated);
    let baseline = approve(directory.path(), &draft);
    let sequence: Value = serde_json::from_slice(&impact(&baseline, None)).unwrap();
    assert_eq!(sequence["reaches_target"], true);
}

#[test]
fn dependency_cli_unresolved_inputs_require_explicit_human_acknowledgement() {
    let directory = TempDir::new().unwrap();
    let manifest =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dependencies/unsupported.toml");
    let output = newton()
        .args(["dependency", "discover", "--map"])
        .arg(example("map.json"))
        .arg("--manifest")
        .arg(manifest)
        .arg("--catalog")
        .arg(example("catalog.json"))
        .args(["--owner", "app"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(review["map"]["issues"].as_array().unwrap().len(), 2);
    assert!(review["discovery_report"]["dependencies"]
        .as_array()
        .unwrap()
        .is_empty());
    let map = directory.path().join("unresolved.json");
    fs::write(&map, output).unwrap();
    let human = human_review(directory.path(), &review, false);
    let baseline = directory.path().join("baseline.json");
    newton()
        .args(["dependency", "approve", "--map"])
        .arg(&map)
        .arg("--review")
        .arg(&human)
        .arg("--output")
        .arg(&baseline)
        .assert()
        .failure();
    assert!(!baseline.exists());
    human_review(directory.path(), &review, true);
    newton()
        .args(["dependency", "approve", "--map"])
        .arg(&map)
        .arg("--review")
        .arg(&human)
        .arg("--output")
        .arg(&baseline)
        .assert()
        .success();
    let sequence: Value = serde_json::from_slice(&impact(&baseline, None)).unwrap();
    assert_eq!(
        sequence["discovery_limitations"].as_array().unwrap().len(),
        2
    );
    newton()
        .args(["dependency", "discover", "--map"])
        .arg(example("map.json"))
        .arg("--manifest")
        .arg(example("Cargo.toml"))
        .arg("--catalog")
        .arg(example("catalog.json"))
        .args(["--owner", "app", "--ecosystem", "npm"])
        .assert()
        .failure();
}

#[test]
fn dependency_cli_suggested_edges_do_not_drive_approved_plans() {
    let directory = TempDir::new().unwrap();
    let mut map: Value = serde_json::from_slice(&fs::read(example("map.json")).unwrap()).unwrap();
    map["dependencies"][1]["discovery"] =
        json!({"kind":"suggested", "method":"fixture-agent", "confidence":1.0});
    let path = directory.path().join("suggested.json");
    fs::write(&path, serde_json::to_vec(&map).unwrap()).unwrap();
    let baseline = approve(directory.path(), &path);
    let sequence: Value = serde_json::from_slice(&impact(&baseline, None)).unwrap();
    assert_eq!(sequence["reaches_target"], false);
    assert!(sequence["stages"].as_array().unwrap().is_empty());
}

#[test]
fn dependency_cli_cycles_remain_explicit_co_release_groups() {
    let directory = TempDir::new().unwrap();
    let mut map: Value = serde_json::from_slice(&fs::read(example("map.json")).unwrap()).unwrap();
    map["dependencies"].as_array_mut().unwrap().push(json!({
        "from": "base", "to": "middle", "kind": "runtime", "constraint": {"kind":"any"},
        "discovery": {"kind":"declared", "by":"authorized-human-test-fixture"}
    }));
    let path = directory.path().join("cycle.json");
    fs::write(&path, serde_json::to_vec(&map).unwrap()).unwrap();
    let baseline = approve(directory.path(), &path);
    let sequence: Value = serde_json::from_slice(&impact(&baseline, None)).unwrap();
    assert_eq!(sequence["stages"].as_array().unwrap().len(), 2);
    let group = &sequence["stages"][0]["groups"][0];
    assert_eq!(group["requires_co_release_resolution"], true);
    assert_eq!(
        group["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|member| member["artifact"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["base", "middle"]
    );
}

#[test]
fn dependency_planner_tools_are_registered_but_human_approval_is_not() {
    use cli_framework::mcp::{McpToolExportPolicy, McpToolRegistry};
    use newton_cli::cli::framework_setup::{build_mcp_command_registry, enumerate_tree_commands};
    let registry = build_mcp_command_registry().unwrap();
    let tools = McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    );
    for name in ["newton_dependency_inspect", "newton_dependency_impact"] {
        assert!(
            tools.resolve_tool(name).is_some(),
            "missing planner tool {name}"
        );
    }
    for name in ["newton_dependency_approve", "newton_dependency_discover"] {
        assert!(
            tools.resolve_tool(name).is_none(),
            "unexpected agent tool {name}"
        );
    }
    let commands = enumerate_tree_commands();
    assert!(
        !commands
            .iter()
            .find(|(path, _)| path == "dependency/approve")
            .unwrap()
            .1
            .expose_chat
    );
}

#[tokio::test]
async fn dependency_mcp_returns_the_actual_inspection_and_impact_json() {
    use cli_framework::mcp::{
        dispatch_tool_call, McpToolExportPolicy, McpToolRegistry, McpTransportKind,
    };
    use newton_cli::cli::framework_setup::build_mcp_command_registry;
    let directory = TempDir::new().unwrap();
    let baseline = approve(directory.path(), &example("map.json"));
    let registry = build_mcp_command_registry().unwrap();
    let tools = McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    );
    let inspected = dispatch_tool_call(
        &tools,
        "newton_dependency_inspect",
        Some(
            json!({"map": example("map.json")})
                .as_object()
                .unwrap()
                .clone(),
        ),
        McpTransportKind::Stdio,
    )
    .await
    .unwrap();
    let expected = inspect(&example("map.json"));
    assert_eq!(inspected.structured_content, Some(expected.clone()));
    let text = serde_json::to_value(&inspected).unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
    let result = dispatch_tool_call(
        &tools,
        "newton_dependency_impact",
        Some(
            json!({"baseline": baseline, "changed":"base", "target":"product"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        McpTransportKind::Http,
    )
    .await
    .unwrap();
    let expected: Value = serde_json::from_slice(&impact(&baseline, None)).unwrap();
    assert_eq!(result.structured_content, Some(expected.clone()));
    let text = serde_json::to_value(&result).unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
}

//! Release-path tests use the distributed definition and actual workflow engine.

use super::{journal, newton as unisolated_newton, setup};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

const GIT_LOCATION_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

fn clear_git_location_env(command: &mut Command) {
    for variable in GIT_LOCATION_ENV_VARS {
        command.env_remove(variable);
    }
}

fn hermetic_command(program: &str) -> Command {
    let mut command = Command::new(program);
    clear_git_location_env(&mut command);
    command
}

fn newton() -> assert_cmd::Command {
    let mut command = unisolated_newton();
    for variable in GIT_LOCATION_ENV_VARS {
        command.env_remove(variable);
    }
    command
}

#[test]
fn resume_uses_pinned_workflows_and_helpers_after_source_changes_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let definition_path = dir.path().join("definition.yaml");
    let source = fs::read_to_string(&definition_path)
        .unwrap()
        .replace("max_cycles: 2", "max_cycles: 3")
        + "\nassets: [snapshot_guard.py]\n";
    fs::write(&definition_path, source).unwrap();
    let fixtures = super::support::fixture_path("optimization");
    fs::copy(
        fixtures.join("snapshot_plan.yaml"),
        dir.path().join("plan.yaml"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("snapshot_guard.py"),
        dir.path().join("snapshot_guard.py"),
    )
    .unwrap();
    fs::write(dir.path().join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.reject=true\n").unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let first = journal(dir.path());
    let run_id = first["run_id"].as_str().unwrap();
    for name in [
        "definition.yaml",
        "grade.yaml",
        "plan.yaml",
        "develop.yaml",
        "snapshot_guard.py",
    ] {
        fs::write(dir.path().join(name), "invalid workflow source").unwrap();
    }
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--once",
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .success();
    assert_eq!(journal(dir.path())["cycle"], 2);
    for name in [
        "definition.yaml",
        "grade.yaml",
        "plan.yaml",
        "develop.yaml",
        "snapshot_guard.py",
    ] {
        fs::remove_file(dir.path().join(name)).unwrap();
    }
    let update = dir.path().join("requirements-update.yaml");
    fs::write(
        &update,
        fs::read_to_string(&update)
            .unwrap()
            .replace("base_revision: 1", "base_revision: 2"),
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--once",
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .success();
    assert_eq!(journal(dir.path())["cycle"], 3);
    let output = newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--resume", run_id, "--inspect"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspection: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        inspection["definition_snapshot"]["files"]["snapshot_guard.py"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(inspection["definition_root"]
        .as_str()
        .unwrap()
        .contains(run_id));
}

#[test]
fn preflight_checks_installed_helper_not_embedded_template() {
    let dir = tempfile::tempdir().unwrap();
    newton()
        .args([
            "--log-dir",
            dir.path().to_str().unwrap(),
            "init",
            dir.path().to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();
    fs::write(dir.path().join(".newton/configs/default.conf"),
        "definition_file=.newton/definitions/software-security/definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n").unwrap();
    fs::write(
        dir.path()
            .join(".newton/definitions/software-security/security.py"),
        "import sys\nsys.exit('installed helper rejected preflight')\n",
    )
    .unwrap();
    for flag in ["--preflight", "--once"] {
        newton()
            .current_dir(dir.path())
            .args(["optimize", "default", flag])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "installed helper rejected preflight",
            ));
    }
    assert!(!dir.path().join(".newton/state/optimize").exists());
    assert!(!dir.path().join(".newton/optimize/claim").exists());
}

#[test]
fn snapshot_tampering_and_path_escapes_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let before = super::run_rejected_cycle(dir.path());
    let run_id = before["run_id"].as_str().unwrap();
    let snapshot = Path::new(before["definition_root"].as_str().unwrap()).join("grade.yaml");
    // Deliberate owner tampering, bypassing the accidental-write protection.
    fs::remove_file(&snapshot).unwrap();
    fs::write(snapshot, "tampered evaluator").unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--resume", run_id, "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("snapshot integrity mismatch"));
    assert_eq!(
        journal(dir.path())["evaluation_count"],
        before["evaluation_count"]
    );

    for reference in ["../outside.py", "/tmp/outside.py"] {
        let fresh = tempfile::tempdir().unwrap();
        setup(fresh.path());
        let definition = fresh.path().join("definition.yaml");
        let source =
            fs::read_to_string(&definition).unwrap() + &format!("\nassets: [{reference:?}]\n");
        fs::write(definition, source).unwrap();
        newton()
            .current_dir(fresh.path())
            .args(["optimize", "demo", "--preflight"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("relative path without traversal"));
        assert!(!fresh.path().join(".newton/state/optimize").exists());
    }
}

#[test]
fn builtin_init_distributes_reusable_definition_and_inspect_has_no_run_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    newton()
        .args([
            "--log-dir",
            dir.path().to_str().unwrap(),
            "init",
            dir.path().to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();
    let definition = dir
        .path()
        .join(".newton/definitions/software-security/definition.yaml");
    assert!(definition.is_file());
    assert!(definition.with_file_name("security.py").is_file());
    let config = dir.path().join(".newton/configs/default.conf");
    let before = fs::read(&config).unwrap();
    let output = newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "default",
            "--inspect",
            "--param",
            "model=\"override\"",
            "--param",
            "agent=\"codex\"",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspection: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(inspection["parameters"]["model"], "override");
    assert_eq!(fs::read(config).unwrap(), before);
    assert!(!dir.path().join(".newton/state/optimize").exists());
    assert!(!dir.path().join(".newton/optimize/claim").exists());
}

#[test]
fn aikit_template_install_includes_the_same_security_definition() {
    let dir = tempfile::tempdir().unwrap();
    let template = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/newton-template")
        .canonicalize()
        .unwrap();
    newton()
        .args([
            "--log-dir",
            dir.path().to_str().unwrap(),
            "init",
            dir.path().to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
        ])
        .assert()
        .success();
    let installed = dir
        .path()
        .join(".newton/definitions/software-security/definition.yaml");
    assert_eq!(
        fs::read(installed).unwrap(),
        fs::read(template.join("newton/definitions/software-security/definition.yaml")).unwrap()
    );
    let develop = fs::read_to_string(dir.path().join(".newton/workflows/develop.yaml")).unwrap();
    for forbidden in [
        "git push",
        "pr_create",
        "pr_approve",
        "project_item_set_status",
        "status: \"In progress\"",
        "MERGED",
    ] {
        assert!(
            !develop.contains(forbidden),
            "distributed develop workflow must stop at a local candidate, found {forbidden}"
        );
    }
    assert!(develop.contains("id: candidate_ready"));
}

#[test]
fn one_definition_binds_two_repositories_with_project_then_run_precedence() {
    let workspace = tempfile::tempdir().unwrap();
    setup(workspace.path());
    for name in ["first", "second"] {
        fs::create_dir(workspace.path().join(name)).unwrap();
        fs::write(
            workspace
                .path()
                .join(format!(".newton/configs/{name}.conf")),
            format!(
                "project_root={name}\ndefinition_file=definition.yaml\nparameter.reject=true\n"
            ),
        )
        .unwrap();
        let output = newton()
            .current_dir(workspace.path())
            .args(["optimize", name, "--inspect", "--param", "reject=false"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let binding: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            binding["context"]["root"],
            workspace.path().join(name).to_str().unwrap()
        );
        assert_eq!(binding["parameters"]["reject"], false);
    }
    assert!(!workspace.path().join(".newton/state/optimize").exists());
}

#[test]
fn missing_prerequisite_fails_before_run_creation() {
    let dir = tempfile::tempdir().unwrap();
    newton()
        .args([
            "--log-dir",
            dir.path().to_str().unwrap(),
            "init",
            dir.path().to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();
    fs::write(dir.path().join(".newton/configs/default.conf"),
        "definition_file=.newton/definitions/software-security/definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.scanner_command=[\"newton-missing-scanner-fixture\"]\n").unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "default", "--preflight"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "missing newton-missing-scanner-fixture",
        ));
    assert!(!dir.path().join(".newton/state/optimize").exists());
    assert!(!dir.path().join(".newton/optimize/claim").exists());
}

#[test]
fn mid_cycle_budget_exhaustion_is_not_an_operational_failure() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let definition = dir.path().join("definition.yaml");
    fs::write(
        &definition,
        fs::read_to_string(&definition)
            .unwrap()
            .replace("max_work: 8", "max_work: 1"),
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = journal(dir.path());
    assert_eq!(run["outcome"]["stop_reason"], "resource_limit");
    assert_eq!(run["work_count"], 1);
    assert_eq!(run["accepted"]["candidate"]["artifact_id"], "original");
}

#[cfg(unix)]
#[test]
fn cancellation_records_uncertainty_and_cannot_be_blindly_resumed() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    fs::copy(
        super::support::fixture_path("optimization/slow_grade.yaml"),
        dir.path().join("grade.yaml"),
    )
    .unwrap();
    fs::write(dir.path().join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n").unwrap();
    let mut child = hermetic_command(env!("CARGO_BIN_EXE_newton"))
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        let runs = dir.path().join(".newton/state/optimize");
        if runs.exists()
            && fs::read_dir(&runs)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    fs::read(entry.path().join("journal.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .is_some_and(|value| value["evaluation_count"] == 1)
                })
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "run never dispatched its grader"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let stopped = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if stopped.elapsed() >= Duration::from_secs(15) {
            let state = journal(dir.path());
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("cancelled optimize did not stop within fifteen seconds: {state}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let run = journal(dir.path());
    assert_eq!(run["outcome"]["stop_reason"], "cancelled");
    assert_eq!(run["requires_reconciliation"], true);
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run["run_id"].as_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Reconcile"));
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = hermetic_command("git");
    let result = command
        .current_dir(root)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

fn repository(root: &Path) -> String {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "--quiet"]);
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("Cargo.lock"), "version = 3\n# vulnerable\n").unwrap();
    git(root, &["add", "Cargo.toml", "Cargo.lock"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "test: create disposable fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

#[cfg(unix)]
#[test]
fn shipped_security_definition_rejects_manifest_test_selection_bypass() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("project");
    repository(&root);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn acceptance_test_must_run() { panic!(\"intentional fixture failure\"); }\n}\n",
    )
    .unwrap();
    assert!(hermetic_command("cargo")
        .current_dir(&root)
        .args(["generate-lockfile", "--offline"])
        .status()
        .unwrap()
        .success());
    let lock = root.join("Cargo.lock");
    fs::write(
        &lock,
        fs::read_to_string(&lock).unwrap() + "\n# vulnerable\n",
    )
    .unwrap();
    git(&root, &["add", "src/lib.rs", "Cargo.lock"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--amend",
            "--no-edit",
            "--quiet",
        ],
    );
    let head = git(&root, &["rev-parse", "HEAD"]);

    let db = workspace.path().join("advisory-db");
    let revision = repository(&db);
    newton()
        .args([
            "--log-dir",
            workspace.path().to_str().unwrap(),
            "init",
            root.to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();

    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let agent = bin.join("codex");
    fs::write(&agent, "#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    let scanner = workspace.path().join("scanner.py");
    fs::copy(
        super::support::fixture_path("optimization/cargo_audit.py"),
        &scanner,
    )
    .unwrap();
    let adversary = super::support::fixture_path("optimization/remediate_disable_lib_tests.py");
    let definition = root.join(".newton/definitions/software-security/definition.yaml");
    let develop = definition.with_file_name("develop.yaml");
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&develop).unwrap()).unwrap();
    let params = document["workflow"]["tasks"][2]["params"]
        .as_mapping_mut()
        .unwrap();
    params.insert(
        serde_yaml::Value::from("engine"),
        serde_yaml::Value::from("command"),
    );
    params.insert(
        serde_yaml::Value::from("engine_command"),
        serde_yaml::to_value(vec!["python3", adversary.to_str().unwrap()]).unwrap(),
    );
    fs::write(develop, serde_yaml::to_string(&document).unwrap()).unwrap();
    fs::write(
        root.join(".newton/configs/default.conf"),
        format!(
            "definition_file={}\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.agent=codex\nparameter.model=fixture\nparameter.advisory_db={}\nparameter.advisory_db_revision={}\nparameter.scanner_command={}\nparameter.test_command=[\"cargo\",\"test\",\"--locked\"]\n",
            definition.display(),
            db.display(),
            revision,
            json!(["python3", scanner.to_str().unwrap()])
        ),
    )
    .unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths).unwrap();

    newton()
        .current_dir(&root)
        .env("PATH", path)
        .args(["optimize", "default", "--once"])
        .assert()
        .success();
    let run = journal(&root);
    assert!(run["accepted"].is_null());
    let test_log = fs::read_to_string(
        run["evidence"]["constraints"]["tests_pass"]["evidence"][0]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        run["evidence"]["constraints"]["tests_pass"]["status"], "satisfied",
        "the adversarial Cargo manifest should make cargo test skip the failing library test: {test_log}"
    );
    assert_eq!(
        run["evidence"]["constraints"]["dependency_files_only"]["status"],
        "violated"
    );
    assert!(
        run["evidence"]["constraints"]["dependency_files_only"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                .as_str()
                .is_some_and(|text| text.contains("only dependency tables")))
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}

#[cfg(unix)]
#[test]
fn shipped_security_definition_rejects_untracked_ancestor_cargo_config() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("project");
    repository(&root);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn acceptance_test_must_run() { panic!(\"intentional fixture failure\"); }\n}\n",
    )
    .unwrap();
    assert!(hermetic_command("cargo")
        .current_dir(&root)
        .args(["generate-lockfile", "--offline"])
        .status()
        .unwrap()
        .success());
    let lock = root.join("Cargo.lock");
    fs::write(
        &lock,
        fs::read_to_string(&lock).unwrap() + "\n# vulnerable\n",
    )
    .unwrap();
    git(&root, &["add", "src/lib.rs", "Cargo.lock"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--amend",
            "--no-edit",
            "--quiet",
        ],
    );
    let head = git(&root, &["rev-parse", "HEAD"]);

    // Before the adversary plants an ancestor runner, the real Cargo command
    // executes the fixture's failing test.
    assert!(!hermetic_command("cargo")
        .current_dir(&root)
        .args(["test", "--locked", "--quiet"])
        .status()
        .unwrap()
        .success());

    let db = workspace.path().join("advisory-db");
    let revision = repository(&db);
    newton()
        .args([
            "--log-dir",
            workspace.path().to_str().unwrap(),
            "init",
            root.to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();

    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let agent = bin.join("codex");
    fs::write(&agent, "#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    let scanner = workspace.path().join("scanner.py");
    fs::copy(
        super::support::fixture_path("optimization/cargo_audit.py"),
        &scanner,
    )
    .unwrap();
    let adversary =
        super::support::fixture_path("optimization/remediate_untracked_cargo_runner.py");
    let definition = root.join(".newton/definitions/software-security/definition.yaml");
    let develop = definition.with_file_name("develop.yaml");
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&develop).unwrap()).unwrap();
    let params = document["workflow"]["tasks"][2]["params"]
        .as_mapping_mut()
        .unwrap();
    params.insert(
        serde_yaml::Value::from("engine"),
        serde_yaml::Value::from("command"),
    );
    params.insert(
        serde_yaml::Value::from("engine_command"),
        serde_yaml::to_value(vec!["python3", adversary.to_str().unwrap()]).unwrap(),
    );
    fs::write(develop, serde_yaml::to_string(&document).unwrap()).unwrap();
    fs::write(
        root.join(".newton/configs/default.conf"),
        format!(
            "definition_file={}\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.agent=codex\nparameter.model=fixture\nparameter.advisory_db={}\nparameter.advisory_db_revision={}\nparameter.scanner_command={}\nparameter.test_command=[\"cargo\",\"test\",\"--locked\",\"--quiet\"]\n",
            definition.display(),
            db.display(),
            revision,
            json!(["python3", scanner.to_str().unwrap()])
        ),
    )
    .unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths).unwrap();

    newton()
        .current_dir(&root)
        .env("PATH", path)
        .args(["optimize", "default", "--once"])
        .assert()
        .failure();
    let run = journal(&root);
    assert!(run["accepted"].is_null());
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert!(root.join(".cargo/config.toml").is_file());
    assert!(
        hermetic_command("cargo")
            .current_dir(&root)
            .args(["test", "--locked", "--quiet"])
            .status()
            .unwrap()
            .success(),
        "the planted runner must reproduce the bypass against the original checkout"
    );
}

#[cfg(unix)]
#[test]
fn shipped_security_preflight_rejects_symlinked_cargo_control_inputs() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    for relative in ["Cargo.toml", "Cargo.lock"] {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("project");
        repository(&root);
        let external = workspace.path().join(format!("external-{relative}"));
        fs::write(&external, fs::read(root.join(relative)).unwrap()).unwrap();
        fs::remove_file(root.join(relative)).unwrap();
        symlink(&external, root.join(relative)).unwrap();
        git(&root, &["add", relative]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "test: symlink Cargo input",
            ],
        );

        let db = workspace.path().join("advisory-db");
        let revision = repository(&db);
        newton()
            .args([
                "--log-dir",
                workspace.path().to_str().unwrap(),
                "init",
                root.to_str().unwrap(),
                "--template",
                "builtin",
            ])
            .assert()
            .success();
        let bin = workspace.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let agent = bin.join("codex");
        fs::write(&agent, "#!/bin/sh\nexit 99\n").unwrap();
        fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
        let scanner = workspace.path().join("scanner.py");
        fs::copy(
            super::support::fixture_path("optimization/cargo_audit.py"),
            &scanner,
        )
        .unwrap();
        fs::write(
            root.join(".newton/configs/default.conf"),
            format!(
                "definition_file=.newton/definitions/software-security/definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.agent=codex\nparameter.model=fixture\nparameter.advisory_db={}\nparameter.advisory_db_revision={}\nparameter.scanner_command={}\nparameter.test_command=[\"cargo\",\"test\",\"--locked\"]\n",
                db.display(),
                revision,
                json!(["python3", scanner.to_str().unwrap()])
            ),
        )
        .unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));

        newton()
            .current_dir(&root)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .args(["optimize", "default", "--preflight"])
            .assert()
            .failure()
            .stderr(predicates::str::contains(format!(
                "{relative} must be a committed regular file, not a symlink"
            )));
        assert!(!root.join(".newton/state/optimize").exists());
    }
}

#[cfg(unix)]
#[test]
fn shipped_security_definition_evaluates_two_repositories_without_touching_heads() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = tempfile::tempdir().unwrap();
    let original = repository(&workspace.path().join("first"));
    let second = repository(&workspace.path().join("second"));
    let db = workspace.path().join("advisory-db");
    let revision = repository(&db);
    let first = workspace.path().join("first");
    newton()
        .args([
            "--log-dir",
            workspace.path().to_str().unwrap(),
            "init",
            first.to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();
    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let agent = bin.join("codex");
    fs::write(&agent, "#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    let scanner = workspace.path().join("scanner.py");
    fs::copy(
        super::support::fixture_path("optimization/cargo_audit.py"),
        &scanner,
    )
    .unwrap();
    let remediate = workspace.path().join("remediate.py");
    fs::copy(
        super::support::fixture_path("optimization/remediate.py"),
        &remediate,
    )
    .unwrap();
    let source = first.join(".newton/definitions/software-security/definition.yaml");
    // Replace only the external agent boundary; execute the distributed graph,
    // evaluator, snapshot adapter and native acceptance policy unchanged.
    let develop = source.with_file_name("develop.yaml");
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&develop).unwrap()).unwrap();
    let params = document["workflow"]["tasks"][2]["params"]
        .as_mapping_mut()
        .unwrap();
    params.insert(
        serde_yaml::Value::from("engine"),
        serde_yaml::Value::from("command"),
    );
    params.insert(
        serde_yaml::Value::from("engine_command"),
        serde_yaml::to_value(vec!["python3", remediate.to_str().unwrap()]).unwrap(),
    );
    fs::write(develop, serde_yaml::to_string(&document).unwrap()).unwrap();
    for (name, head) in [("first", original), ("second", second)] {
        let root = workspace.path().join(name);
        fs::create_dir_all(root.join(".newton/configs")).unwrap();
        let config = format!("definition_file={}\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\nparameter.agent=codex\nparameter.model=fixture\nparameter.advisory_db={}\nparameter.advisory_db_revision={}\nparameter.scanner_command={}\nparameter.test_command=[\"true\"]\n", source.display(), db.display(), revision,
            json!(["python3", scanner.to_str().unwrap()]));
        fs::write(root.join(".newton/configs/default.conf"), &config).unwrap();
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).unwrap();
        newton()
            .current_dir(&root)
            .env("PATH", &path)
            .args(["optimize", "default", "--preflight"])
            .assert()
            .success();
        assert!(!root.join(".newton/state/optimize").exists());
        assert!(!root.join(".newton/optimize-artifacts").exists());
        newton()
            .current_dir(&root)
            .env("PATH", &path)
            .args(["optimize", "default", "--once"])
            .assert()
            .success();
        let run = journal(&root);
        assert_eq!(run["outcome"]["stop_reason"], "completed");
        let accepted = run["accepted"]["candidate"]["artifact_id"]
            .as_str()
            .unwrap();
        assert_ne!(accepted, head);
        assert!(git(&root, &["show", &format!("{accepted}:Cargo.lock")]).contains("fixed"));
        assert!(git(&root, &["show", &format!("{accepted}:Cargo.toml")])
            .contains("fixture-safe-dependency"));
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
        assert!(
            !git(&root, &["worktree", "list", "--porcelain"])
                .contains("newton-security-evaluation-"),
            "isolated evaluation worktrees must be removed after evidence is captured"
        );
        assert_eq!(run["work_count"], 2);
        assert_eq!(run["evaluation_count"], 2);
        assert!(fs::read_to_string(root.join("Cargo.lock"))
            .unwrap()
            .contains("vulnerable"));
        assert_eq!(
            fs::read_to_string(root.join(".newton/configs/default.conf")).unwrap(),
            config
        );
    }
}

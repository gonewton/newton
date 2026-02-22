use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn set_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn write_script(path: &Path, body: &str) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let mut file = File::create(path)?;
    writeln!(file, "{}", body)?;
    set_executable(path)?;
    Ok(())
}

fn setup_profile_workspace(workspace: &Path) -> std::io::Result<PathBuf> {
    let configs_dir = workspace.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let project_root = workspace.join("project-root");
    let project_newton = project_root.join(".newton");
    fs::create_dir_all(project_newton.join("scripts"))?;
    fs::create_dir_all(project_newton.join("hooks"))?;

    fs::create_dir_all(workspace.join(".newton").join("scripts"))?;

    let pre_hook = project_newton.join("hooks").join("pre_run.sh");
    write_script(
        &pre_hook,
        r#"#!/bin/sh
echo "$NEWTON_PROJECT_ID" > .newton/hooks/pre_run_env.txt
exit 0"#,
    )?;

    let post_success = project_newton.join("hooks").join("post_success.sh");
    write_script(
        &post_success,
        r#"#!/bin/sh
echo "$NEWTON_EXECUTOR_CODING_AGENT_MODEL" > .newton/hooks/post_success_env.txt
exit 0"#,
    )?;

    let post_fail = project_newton.join("hooks").join("post_fail.sh");
    write_script(
        &post_fail,
        r#"#!/bin/sh
echo "post-fail" > .newton/hooks/post_fail_env.txt
exit 0"#,
    )?;

    let evaluator = project_newton.join("scripts").join("evaluator.sh");
    write_script(
        &evaluator,
        r#"#!/bin/sh
if [ "$NEWTON_EXECUTOR_CODING_AGENT_MODEL" != "test-model" ]; then
  exit 1
fi
if [ -n "$NEWTON_CONTROL_FILE" ]; then
  printf '%s\n' '{"done": true}' > "$NEWTON_CONTROL_FILE"
fi
exit 0"#,
    )?;

    let advisor = project_newton.join("scripts").join("advisor.sh");
    write_script(
        &advisor,
        r#"#!/bin/sh
exit 0"#,
    )?;

    let executor = workspace
        .join(".newton")
        .join("scripts")
        .join("executor.sh");
    write_script(
        &executor,
        r#"#!/bin/sh
exit 0"#,
    )?;

    let conf_contents = r#"project_root = ./project-root
coding_agent = test-agent
coding_model = test-model
pre_run_script = .newton/hooks/pre_run.sh
post_success_script = .newton/hooks/post_success.sh
post_fail_script = .newton/hooks/post_fail.sh
control_file = custom_control.json
max_iterations = 2
max_time = 60
"#;
    fs::write(configs_dir.join("newton.conf"), conf_contents)?;

    Ok(project_root)
}

#[test]
fn run_profile_mode_applies_config_and_hooks() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace = workspace_dir.path();
    setup_profile_workspace(workspace)?;

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("newton");
    cmd.current_dir(workspace).arg("run").arg("newton");
    cmd.assert().success();

    let pre_hook_file = workspace
        .join("project-root")
        .join(".newton")
        .join("hooks")
        .join("pre_run_env.txt");
    let pre_contents = fs::read_to_string(pre_hook_file)?;
    assert_eq!(pre_contents.trim(), "newton");

    let post_success_file = workspace
        .join("project-root")
        .join(".newton")
        .join("hooks")
        .join("post_success_env.txt");
    let post_contents = fs::read_to_string(post_success_file)?;
    assert_eq!(post_contents.trim(), "test-model");

    let post_fail_file = workspace
        .join("project-root")
        .join(".newton")
        .join("hooks")
        .join("post_fail_env.txt");
    assert!(!post_fail_file.exists());

    let control_file = workspace.join("project-root").join("custom_control.json");
    let control_contents = fs::read_to_string(control_file)?;
    assert!(control_contents.contains("{\"done\": true}"));

    Ok(())
}

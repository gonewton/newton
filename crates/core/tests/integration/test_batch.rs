use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn create_stub_tool_scripts(workspace_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = workspace_path.join("project-root");
    let project_scripts = project_root.join(".newton").join("scripts");
    fs::create_dir_all(&project_scripts)?;

    let evaluator = project_scripts.join("evaluator.sh");
    fs::write(
        &evaluator,
        r#"#!/bin/sh
set -e
if [ -n "$NEWTON_CONTROL_FILE" ]; then
  printf '%s\n' '{"done": true}' > "$NEWTON_CONTROL_FILE"
fi
exit 0
"#,
    )?;
    set_executable(&evaluator)?;

    let advisor = project_scripts.join("advisor.sh");
    fs::write(&advisor, "#!/bin/sh\nexit 0\n")?;
    set_executable(&advisor)?;

    let workspace_scripts = workspace_path.join(".newton").join("scripts");
    fs::create_dir_all(&workspace_scripts)?;

    let executor = workspace_scripts.join("executor.sh");
    fs::write(&executor, "#!/bin/sh\nexit 0\n")?;
    set_executable(&executor)?;

    let coder = workspace_scripts.join("coder.sh");
    fs::write(&coder, "#!/bin/sh\nexit 0\n")?;
    set_executable(&coder)?;

    Ok(())
}

fn set_executable(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[test]
fn batch_once_moves_plan_to_completed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();
    create_stub_tool_scripts(workspace_path)?;

    let project_root = workspace_path.join("project-root");
    fs::create_dir_all(project_root.join(".newton"))?;

    let configs_dir = workspace_path.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let plan_todo_dir = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("todo");
    fs::create_dir_all(&plan_todo_dir)?;
    let plan_item = plan_todo_dir.join("item.plan");
    fs::write(&plan_item, "update goal")?;

    let conf_path = configs_dir.join("proj.conf");
    let config_contents = r#"
        project_root = ./project-root
        coding_agent = test-agent
        coding_model = test-model
    "#;
    fs::write(&conf_path, config_contents)?;

    let mut cmd = Command::cargo_bin("newton")?;
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(workspace_path)
        .arg("--once");
    cmd.assert().success();

    let completed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("completed")
        .join("item.plan");
    assert!(completed.exists());
    assert!(!plan_item.exists());

    Ok(())
}

#[test]
fn batch_success_script_moves_plan_to_completed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();
    create_stub_tool_scripts(workspace_path)?;

    let project_root = workspace_path.join("project-root");
    fs::create_dir_all(project_root.join(".newton/hooks"))?;

    let configs_dir = workspace_path.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let plan_todo_dir = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("todo");
    fs::create_dir_all(&plan_todo_dir)?;
    let plan_item = plan_todo_dir.join("item.plan");
    fs::write(&plan_item, "update goal")?;

    let conf_path = configs_dir.join("proj.conf");
    let config_contents = r#"
        project_root = ./project-root
        coding_agent = test-agent
        coding_model = test-model
        post_success_script = touch .newton/hooks/success.txt
    "#;
    fs::write(&conf_path, config_contents)?;

    let mut cmd = Command::cargo_bin("newton")?;
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(workspace_path)
        .arg("--once");
    cmd.assert().success();

    let completed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("completed")
        .join("item.plan");
    let failed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("failed")
        .join("item.plan");
    assert!(completed.exists());
    assert!(!failed.exists());
    assert!(project_root.join(".newton/hooks/success.txt").exists());
    assert!(!plan_item.exists());

    Ok(())
}

#[test]
fn batch_success_script_failure_moves_plan_to_failed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();
    create_stub_tool_scripts(workspace_path)?;

    let project_root = workspace_path.join("project-root");
    fs::create_dir_all(project_root.join(".newton"))?;

    let configs_dir = workspace_path.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let plan_todo_dir = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("todo");
    fs::create_dir_all(&plan_todo_dir)?;
    let plan_item = plan_todo_dir.join("item.plan");
    fs::write(&plan_item, "update goal")?;

    let conf_path = configs_dir.join("proj.conf");
    let config_contents = r#"
        project_root = ./project-root
        coding_agent = test-agent
        coding_model = test-model
        post_success_script = false
    "#;
    fs::write(&conf_path, config_contents)?;

    let mut cmd = Command::cargo_bin("newton")?;
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(workspace_path)
        .arg("--once");
    cmd.assert().success();

    let completed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("completed")
        .join("item.plan");
    let failed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("failed")
        .join("item.plan");
    assert!(!completed.exists());
    assert!(failed.exists());
    assert!(!plan_item.exists());

    Ok(())
}

#[test]
fn batch_failure_runs_post_fail_script() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();
    create_stub_tool_scripts(workspace_path)?;

    let project_root = workspace_path.join("project-root");
    fs::create_dir_all(project_root.join(".newton/hooks"))?;
    fs::write(
        project_root.join("newton.toml"),
        "[project]\nname = \"\"\n",
    )?;

    let configs_dir = workspace_path.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let plan_todo_dir = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("todo");
    fs::create_dir_all(&plan_todo_dir)?;
    let plan_item = plan_todo_dir.join("item.plan");
    fs::write(&plan_item, "update goal")?;

    let conf_path = configs_dir.join("proj.conf");
    let config_contents = r#"
        project_root = ./project-root
        coding_agent = test-agent
        coding_model = test-model
        post_fail_script = touch .newton/hooks/fail.txt
    "#;
    fs::write(&conf_path, config_contents)?;

    let mut cmd = Command::cargo_bin("newton")?;
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(workspace_path)
        .arg("--once");
    cmd.assert().failure();

    let failed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("failed")
        .join("item.plan");
    assert!(failed.exists());
    assert!(!plan_item.exists());
    assert!(project_root.join(".newton/hooks/fail.txt").exists());

    Ok(())
}

#[test]
fn batch_success_hook_receives_result_env_var() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();
    create_stub_tool_scripts(workspace_path)?;

    let project_root = workspace_path.join("project-root");
    fs::create_dir_all(project_root.join(".newton/hooks"))?;

    let configs_dir = workspace_path.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir)?;

    let plan_todo_dir = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("todo");
    fs::create_dir_all(&plan_todo_dir)?;
    let plan_item = plan_todo_dir.join("item.plan");
    fs::write(&plan_item, "update goal")?;

    let conf_path = configs_dir.join("proj.conf");
    let config_contents = r#"
        project_root = ./project-root
        coding_agent = test-agent
        coding_model = test-model
        post_success_script = '''printf '%s\n' "$NEWTON_RESULT" > .newton/hooks/result-env.txt'''
    "#;
    fs::write(&conf_path, config_contents)?;

    let mut cmd = Command::cargo_bin("newton")?;
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(workspace_path)
        .arg("--once");
    cmd.assert().success();

    let completed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("completed")
        .join("item.plan");
    let failed = workspace_path
        .join(".newton")
        .join("plan")
        .join("proj")
        .join("failed")
        .join("item.plan");
    let hook_file = project_root.join(".newton/hooks/result-env.txt");

    assert!(completed.exists());
    assert!(!failed.exists());
    assert!(!plan_item.exists());

    let env_contents = fs::read_to_string(&hook_file)?;
    assert_eq!(env_contents, "success\n");

    Ok(())
}

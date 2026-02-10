use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn batch_once_moves_plan_to_completed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path();

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

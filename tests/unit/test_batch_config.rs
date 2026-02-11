use newton::core::batch_config::{parse_conf, BatchProjectConfig, find_workspace_root};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn parse_conf_handles_comments_and_whitespace() {
    let temp_dir = TempDir::new().unwrap();
    let conf_path = temp_dir.path().join("project.conf");
    let content = r#"
        # This is a comment
        project_root = ./project

        coding_agent = opencode
        coding_model = glm-4.7
    "#;
    fs::write(&conf_path, content).unwrap();

    let settings = parse_conf(&conf_path).unwrap();
    assert_eq!(settings.get("project_root").unwrap(), "./project");
    assert_eq!(settings.get("coding_agent").unwrap(), "opencode");
    assert_eq!(settings.get("coding_model").unwrap(), "glm-4.7");
}

#[test]
fn parse_conf_requires_keys() {
    let temp_dir = TempDir::new().unwrap();
    let conf_path = temp_dir.path().join("project.conf");
    fs::write(&conf_path, "coding_agent = opencode").unwrap();

    let settings = parse_conf(&conf_path).unwrap();
    assert!(settings.get("project_root").is_none());

    let result = BatchProjectConfig::load(temp_dir.path(), "project");
    assert!(result.is_err());
}

#[test]
fn batch_project_config_resolves_relative_project_root() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert_eq!(config.project_root, project_root);
    assert_eq!(config.coding_agent, "opencode");
}

#[test]
fn batch_project_config_parses_optional_scripts() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
        post_success_script = ./scripts/success.sh
        post_fail_script = ./scripts/fail.sh
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert_eq!(config.post_success_script.as_deref(), Some("./scripts/success.sh"));
    assert_eq!(config.post_fail_script.as_deref(), Some("./scripts/fail.sh"));
}

#[test]
fn batch_project_config_handles_missing_scripts() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
        post_success_script =   
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert!(config.post_success_script.is_none());
    assert!(config.post_fail_script.is_none());
}

#[test]
fn find_workspace_root_climb_upwards() {
    let workspace = TempDir::new().unwrap();
    fs::create_dir_all(workspace.path().join(".newton")).unwrap();
    let nested = workspace.path().join("level").join("sub");
    fs::create_dir_all(&nested).unwrap();

    let found = find_workspace_root(&nested).unwrap();
    assert_eq!(found, workspace.path());
}

#[test]
fn find_workspace_root_missing_fails() {
    let temp_dir = TempDir::new().unwrap();
    let nested = temp_dir.path().join("no_newton");
    fs::create_dir_all(&nested).unwrap();

    assert!(find_workspace_root(&nested).is_err());
}

#[test]
fn batch_project_config_derives_default_tools() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton").join("scripts")).unwrap();
    fs::create_dir_all(workspace.path().join(".newton").join("scripts")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();

    let expected_evaluator = project_root
        .join(".newton")
        .join("scripts")
        .join("evaluator.sh");
    let expected_advisor = project_root
        .join(".newton")
        .join("scripts")
        .join("advisor.sh");
    let expected_executor = workspace
        .join(".newton")
        .join("scripts")
        .join("executor.sh");

    assert_eq!(
        config.evaluator_cmd,
        Some(expected_evaluator.display().to_string())
    );
    assert_eq!(
        config.advisor_cmd,
        Some(expected_advisor.display().to_string())
    );
    assert_eq!(
        config.executor_cmd,
        Some(expected_executor.display().to_string())
    );
}

#[test]
fn batch_project_config_tool_overrides_respect_relative_paths() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton").join("scripts")).unwrap();
    fs::create_dir_all(workspace.path().join(".newton").join("scripts")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
        evaluator_cmd = ./custom/evaluator.sh
        advisor_cmd =
        executor_cmd = tools/executor.sh
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert_eq!(
        config.evaluator_cmd,
        Some(
            project_root
                .join("custom")
                .join("evaluator.sh")
                .display()
                .to_string()
        )
    );
    assert!(config.advisor_cmd.is_none());
    assert_eq!(
        config.executor_cmd,
        Some(
            workspace
                .join("tools")
                .join("executor.sh")
                .display()
                .to_string()
        )
    );
}

#[test]
fn batch_project_config_parses_additional_settings() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton")).unwrap();
    fs::create_dir_all(project_root.join(".newton").join("scripts")).unwrap();
    fs::create_dir_all(workspace.path().join(".newton").join("scripts")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
        resume = true
        verbose = 1
        max_iterations = 8
        max_time = 1200
        control_file = custom_control.json
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert!(config.resume);
    assert!(config.verbose);
    assert_eq!(config.max_iterations, Some(8));
    assert_eq!(config.max_time, Some(1200));
    assert_eq!(config.control_file.as_deref(), Some("custom_control.json"));
}

#[test]
fn batch_project_config_resume_accepts_one() {
    let workspace = TempDir::new().unwrap();
    let project_root = workspace.path().join("workspace-project");
    fs::create_dir_all(project_root.join(".newton")).unwrap();
    fs::create_dir_all(project_root.join(".newton").join("scripts")).unwrap();
    fs::create_dir_all(workspace.path().join(".newton").join("scripts")).unwrap();

    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();
    let conf_path = configs_dir.join("proj.conf");
    let content = r#"
        project_root = ./workspace-project
        coding_agent = opencode
        coding_model = glm-4.7
        resume = 1
    "#;
    fs::write(&conf_path, content).unwrap();

    let config = BatchProjectConfig::load(workspace.path(), "proj").unwrap();
    assert!(config.resume);
}

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

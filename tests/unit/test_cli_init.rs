use newton::cli::{args::InitArgs, commands};
use std::env;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

struct PathGuard {
    original: String,
}

impl PathGuard {
    fn new(original: String) -> Self {
        PathGuard { original }
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        env::set_var("PATH", &self.original);
    }
}

#[tokio::test]
async fn init_command_renders_template_assets() {
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();
    let template_dir = workspace.join(".newton/templates/basic");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("executor.sh"), "#!/bin/bash\necho hi\n").unwrap();
    fs::write(template_dir.join("newton.toml"), "[project]\nname = \"{{project_name}}\"\n").unwrap();

    let current_path = env::var("PATH").unwrap_or_default();
    let bin_path = env::current_dir().unwrap().join("tests/bin");
    env::set_var("PATH", format!("{}:{}", bin_path.display(), current_path));
    let _guard = PathGuard::new(current_path);

    let args = InitArgs {
        workspace_path: workspace.clone(),
        template: Some("basic".to_string()),
        name: Some("DemoProject".to_string()),
        coding_agent: Some("opencode".to_string()),
        model: Some("glm".to_string()),
        interactive: false,
    };

    commands::init(args).await.unwrap();

    assert!(workspace.join(".newton/executor.sh").exists());
    assert!(workspace.join("newton.toml").exists());
    assert!(workspace.join("GOAL.md").exists());
}

use crate::cli::args::InitArgs;
use crate::core::config::ExecutorConfig;
use crate::Result;
use aikit_sdk::{install_template_from_source, InstallTemplateFromSourceOptions, TemplateSource};
use anyhow::anyhow;
use std::fs;
use std::io::Write;
use std::path::Path;

const DEFAULT_TEMPLATE_SOURCE: &str = "gonewton/newton-templates";
const DEFAULT_CODING_AGENT: &str = "opencode";
const DEFAULT_CODING_MODEL: &str = "zai-coding-plan/glm-4.7";

/// Handles `newton init` by creating a `.newton/` workspace and installing the Newton template via aikit-sdk.
pub async fn run(args: InitArgs) -> Result<()> {
    // Resolve target path (default: current directory)
    let path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    // Canonicalize the path to ensure it's absolute
    let path = fs::canonicalize(&path)
        .or_else(|_| {
            // If canonicalize fails (e.g., path doesn't exist), try to create it
            fs::create_dir_all(&path)?;
            fs::canonicalize(&path)
        })
        .map_err(|e| anyhow!("Invalid path: {}", e))?;

    if !path.is_dir() {
        return Err(anyhow!("Path {} is not a directory", path.display()));
    }

    let newton_dir = path.join(".newton");

    // Check if .newton already exists (idempotency check)
    if newton_dir.exists() {
        return Err(anyhow!(
            ".newton already exists at {}; remove it or use a different path",
            path.display()
        ));
    }

    // Create directory layout
    create_directory_layout(&newton_dir)?;

    // Install template using aikit-sdk
    let template_source = args
        .template_source
        .unwrap_or_else(|| DEFAULT_TEMPLATE_SOURCE.to_string());
    install_template(&path, &template_source)?;

    // Create minimal executor.sh stub if it doesn't exist
    ensure_executor_script(&newton_dir)?;

    // Write .newton/configs/default.conf
    write_default_config(&newton_dir, &path)?;

    println!("Initialized Newton workspace at {}", path.display());
    println!("Run: newton run");

    Ok(())
}

/// Creates the required directory layout for a Newton workspace
fn create_directory_layout(newton_dir: &Path) -> Result<()> {
    // Create base directories
    fs::create_dir_all(newton_dir.join("configs"))?;
    fs::create_dir_all(newton_dir.join("tasks"))?;

    // Create plan/default subdirectories
    fs::create_dir_all(newton_dir.join("plan/default/todo"))?;
    fs::create_dir_all(newton_dir.join("plan/default/completed"))?;
    fs::create_dir_all(newton_dir.join("plan/default/failed"))?;
    fs::create_dir_all(newton_dir.join("plan/default/draft"))?;

    // Optionally create .newton/state/ for consistency
    fs::create_dir_all(newton_dir.join("state"))?;

    Ok(())
}

/// Installs the Newton template using aikit-sdk
fn install_template(project_root: &Path, template_source: &str) -> Result<()> {
    let source = TemplateSource::parse(template_source).map_err(|e| {
        anyhow!(
            "Failed to parse template source '{}': {}",
            template_source,
            e
        )
    })?;

    let options = InstallTemplateFromSourceOptions {
        source,
        project_root: project_root.to_path_buf(),
        packages_dir: None, // Use temp directory, don't cache
    };

    install_template_from_source(options).map_err(|e| {
        anyhow!(
            "Failed to install template from source '{}': {}",
            template_source,
            e
        )
    })?;

    Ok(())
}

/// Ensures executor.sh exists, creating a minimal stub if the template didn't provide one
fn ensure_executor_script(newton_dir: &Path) -> Result<()> {
    let executor_path = newton_dir.join("scripts/executor.sh");

    if !executor_path.exists() {
        fs::create_dir_all(executor_path.parent().unwrap())?;

        let stub_content = r#"#!/bin/bash
# Minimal executor stub
# Replace with your actual executor implementation

echo "Executor stub: implement your coding agent integration here"
exit 1
"#;

        fs::write(&executor_path, stub_content)?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&executor_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&executor_path, perms)?;
        }
    }

    Ok(())
}

/// Writes .newton/configs/default.conf with key=value pairs
fn write_default_config(newton_dir: &Path, project_root: &Path) -> Result<()> {
    let config_path = newton_dir.join("configs/default.conf");

    // Load defaults from ExecutorConfig
    let defaults = ExecutorConfig::default();
    let coding_agent = if defaults.coding_agent.is_empty() {
        DEFAULT_CODING_AGENT
    } else {
        &defaults.coding_agent
    };
    let coding_model = if defaults.coding_agent_model.is_empty() {
        DEFAULT_CODING_MODEL
    } else {
        &defaults.coding_agent_model
    };

    let mut config_file = fs::File::create(&config_path)?;

    // Write key=value lines
    writeln!(config_file, "project_root={}", project_root.display())?;
    writeln!(config_file, "coding_agent={}", coding_agent)?;
    writeln!(config_file, "coding_model={}", coding_model)?;
    writeln!(config_file)?;

    // Script paths (optional - defaults to .newton/scripts/*.sh)
    writeln!(
        config_file,
        "# Can be absolute paths or relative to project/workspace root"
    )?;
    writeln!(config_file, "# evaluator_cmd=.newton/scripts/evaluator.sh")?;
    writeln!(config_file, "# advisor_cmd=.newton/scripts/advisor.sh")?;
    writeln!(config_file, "# executor_cmd=.newton/scripts/executor.sh")?;
    writeln!(config_file, "# coder_cmd=.newton/scripts/coder.sh")?;
    writeln!(config_file)?;

    // Optionally add script paths if they exist
    let post_success_script = newton_dir.join("scripts/post-success.sh");
    if post_success_script.exists() {
        writeln!(
            config_file,
            "post_success_script=.newton/scripts/post-success.sh"
        )?;
    }

    let post_fail_script = newton_dir.join("scripts/post-failure.sh");
    if post_fail_script.exists() {
        writeln!(
            config_file,
            "post_fail_script=.newton/scripts/post-failure.sh"
        )?;
    }

    Ok(())
}

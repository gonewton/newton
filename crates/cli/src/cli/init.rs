use crate::cli::args::InitArgs;
use crate::Result;
use aikit_sdk::{install_template_from_source, InstallTemplateFromSourceOptions, TemplateSource};
use anyhow::anyhow;
use newton_core::core::config::ExecutorConfig;
use std::fs;
use std::io::Write;
use std::path::Path;

const DEFAULT_TEMPLATE_SOURCE: &str = "gonewton/newton-templates";
const DEFAULT_CODING_MODEL: &str = "zai-coding-plan/glm-4.7";

/// Handles `newton init` by creating a `.newton/` workspace and installing the Newton template via aikit-sdk.
pub fn run(args: InitArgs) -> Result<()> {
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
        .map_err(|e| anyhow!("Invalid path: {e}"))?;

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
        .template
        .unwrap_or_else(|| DEFAULT_TEMPLATE_SOURCE.to_string());
    if template_source != "builtin" {
        install_template(&path, &template_source)?;
    }
    crate::cli::commands::optimize::assets::install(&newton_dir)?;

    // Write .newton/configs/default.conf
    write_default_config(&newton_dir, &path)?;

    println!("Initialized Newton workspace at {}", path.display());
    println!(
        "Inspect the shipped dependency-security definition with newton optimize default --inspect --workspace {}; configure its prerequisites, then use --preflight before --once",
        path.display()
    );

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
    let source = TemplateSource::parse(template_source)
        .map_err(|e| anyhow!("Failed to parse template source '{template_source}': {e}"))?;

    let options = InstallTemplateFromSourceOptions {
        source,
        project_root: project_root.to_path_buf(),
        packages_dir: None, // Use temp directory, don't cache
    };

    install_template_from_source(options)
        .map_err(|e| anyhow!("Failed to install template from source '{template_source}': {e}"))?;

    Ok(())
}

/// Writes .newton/configs/default.conf with key=value pairs
fn write_default_config(newton_dir: &Path, project_root: &Path) -> Result<()> {
    let config_path = newton_dir.join("configs/default.conf");

    // Load defaults from ExecutorConfig
    let defaults = ExecutorConfig::default();
    let coding_model = if defaults.coding_agent_model.is_empty() {
        DEFAULT_CODING_MODEL
    } else {
        &defaults.coding_agent_model
    };

    let mut config_file = fs::File::create(&config_path)?;

    // Write key=value lines
    writeln!(config_file, "project_root={}", project_root.display())?;
    writeln!(config_file, "coding_model={coding_model}")?;
    writeln!(config_file)?;
    writeln!(
        config_file,
        "# Reusable Rust/Cargo.lock dependency-security definition (trusted host; no auto-promotion)"
    )?;
    writeln!(
        config_file,
        "definition_file=.newton/definitions/software-security/definition.yaml"
    )?;
    writeln!(config_file, "# parameter.agent=pi\n# parameter.model=your-configured-model\n# parameter.advisory_db=/absolute/path/to/rustsec-advisory-db\n# parameter.advisory_db_revision=full-git-commit-id")?;
    writeln!(config_file, "# Unsandboxed agents require explicit trusted-host authority; review optimize --help before granting.")?;

    Ok(())
}

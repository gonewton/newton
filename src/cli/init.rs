use crate::cli::args::InitArgs;
use crate::core::{ContextManager, NewtonConfig, TemplateInfo, TemplateManager, TemplateRenderer};
use crate::Result;
use anyhow::anyhow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

const AIKIT_DOCS_URL: &str = "https://aikit.readthedocs.io";
const DEFAULT_LANGUAGE: &str = "rust";
const DEFAULT_TEMPLATE: &str = "basic";

/// Handles `newton init` by rendering templates, creating state, and scaffolding.
pub async fn run(args: InitArgs) -> Result<()> {
    ensure_aikit_installed()?;

    let workspace_path = args.workspace_path;
    let newton_dir = workspace_path.join(".newton");

    if newton_dir.exists() {
        return Err(anyhow!("Workspace already contains .newton directory"));
    }

    let templates = TemplateManager::list_templates(&workspace_path)?;
    if templates.is_empty() {
        return Err(anyhow!(
            "No templates found under {}/.newton/templates/. Install a template via aikit ({}) before running init.",
            workspace_path.display(),
            AIKIT_DOCS_URL
        ));
    }

    let template_name = select_template(args.template, args.interactive, &templates)?;
    if !templates.iter().any(|t| t.name == template_name) {
        return Err(anyhow!(
            "Template '{}' is not installed. Available templates: {}",
            template_name,
            templates
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let defaults = NewtonConfig::default();
    let project_name = args
        .name
        .or_else(|| {
            workspace_path
                .file_name()
                .and_then(OsStr::to_str)
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| defaults.project.name.clone());
    let coding_agent = args
        .coding_agent
        .unwrap_or_else(|| defaults.executor.coding_agent.clone());
    let coding_agent_model = args
        .model
        .unwrap_or_else(|| defaults.executor.coding_agent_model.clone());
    let test_command = determine_test_command(&workspace_path);
    let language = DEFAULT_LANGUAGE.to_string();

    let project_name = if args.interactive && project_name.trim().is_empty() {
        prompt_for_value("Project name", Some(&project_name))?
    } else {
        project_name
    };

    let coding_agent = if args.interactive {
        prompt_for_value("Coding agent", Some(&coding_agent))?
    } else {
        coding_agent
    };

    let coding_agent_model = if args.interactive {
        prompt_for_value("Coding agent model", Some(&coding_agent_model))?
    } else {
        coding_agent_model
    };

    let language = if args.interactive {
        prompt_for_value("Language", Some(&language))?
    } else {
        language
    };

    let template_to_render = template_name.clone();

    fs::create_dir_all(newton_dir.join("state"))?;
    let context_file = newton_dir.join("state/context.md");
    ContextManager::clear_context(&context_file)?;
    fs::write(newton_dir.join("state/promise.txt"), "")?;
    fs::write(newton_dir.join("state/executor_prompt.md"), "")?;

    let mut variables = HashMap::new();
    variables.insert("project_name".to_string(), project_name.clone());
    variables.insert("coding_agent".to_string(), coding_agent.clone());
    variables.insert("coding_agent_model".to_string(), coding_agent_model.clone());
    variables.insert("test_command".to_string(), test_command.clone());
    variables.insert("language".to_string(), language.clone());

    TemplateRenderer::render_template(&workspace_path, &template_to_render, variables)?;

    let config_path = workspace_path.join("newton.toml");
    if !config_path.exists() {
        let mut config = NewtonConfig::default();
        config.project.name = project_name.clone();
        config.project.template = Some(template_to_render.clone());
        config.executor.coding_agent = coding_agent;
        config.executor.coding_agent_model = coding_agent_model;
        config.evaluator.test_command = Some(test_command.clone());
        fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    }

    let goal_path = workspace_path.join("GOAL.md");
    if !goal_path.exists() {
        fs::write(
            &goal_path,
            format!(
                "# Goal for {}\n\nDescribe what you want Newton Loop to achieve.\n",
                project_name
            ),
        )?;
    }

    println!(
        "Initialized workspace with template '{}'",
        template_to_render
    );
    println!("Newton CLI is ready. See GOAL.md to describe your objective.");

    Ok(())
}

fn ensure_aikit_installed() -> Result<()> {
    match Command::new("aikit").arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(anyhow!(
            "aikit is required for template support. Install it first:\n  {}",
            AIKIT_DOCS_URL
        )),
    }
}

fn select_template(
    requested: Option<String>,
    interactive: bool,
    available: &[TemplateInfo],
) -> Result<String> {
    if let Some(name) = requested {
        return Ok(name);
    }
    if interactive {
        prompt_for_selection("Select a template", available)
    } else {
        Ok(DEFAULT_TEMPLATE.to_string())
    }
}

fn prompt_for_selection(prompt: &str, available: &[TemplateInfo]) -> Result<String> {
    println!("{}:", prompt);
    for (idx, template) in available.iter().enumerate() {
        println!("  {}) {}", idx + 1, template.name);
    }
    let selection = prompt_for_value("Enter the number of the template", None)?;
    let index: usize = selection
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid selection"))?;
    available
        .get(index.saturating_sub(1))
        .ok_or_else(|| anyhow!("Template selection out of range"))
        .map(|t| t.name.clone())
}

fn prompt_for_value(prompt: &str, default: Option<&str>) -> Result<String> {
    print!("{}", prompt);
    if let Some(def) = default {
        print!(" [{}]", def);
    }
    print!(": ");
    io::stdout().flush()?;

    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        if let Some(def) = default {
            return Ok(def.to_string());
        }
        return Err(anyhow!("Value cannot be empty"));
    }
    Ok(trimmed.to_string())
}

fn determine_test_command(workspace_path: &Path) -> String {
    let run_tests = workspace_path.join("scripts/run-tests.sh");
    if run_tests.exists() {
        format!("./{}", run_tests.display())
    } else {
        "cargo test".to_string()
    }
}

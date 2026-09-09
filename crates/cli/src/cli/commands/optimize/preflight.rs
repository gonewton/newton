//! No agent dispatch, run creation, or candidate mutation during prerequisite checks.

use super::workflow::validate_authority;
use anyhow::{Context, Result};
use newton_types::optimization::BoundOptimizationDefinition;
use serde_json::json;
use std::{fs, time::Duration};

pub(super) async fn check(
    binding: &BoundOptimizationDefinition,
    runtime: &super::snapshot::Runtime,
) -> Result<()> {
    for role in ["grade", "plan", "develop"] {
        if !binding.definition.workflows.contains_key(role) {
            anyhow::bail!("definition requires workflow role '{role}'");
        }
    }
    if binding.definition.workflows.contains_key("promote") {
        anyhow::bail!(
            "promotion workflow is unsupported: the generic workflow host cannot independently verify the promoted target state or perform an atomic compare-and-swap; remove workflows.promote and retain the qualified candidate for an external, target-specific promotion boundary"
        );
    }
    let grade = super::workflow::grade_reference(&binding.requirements.requirements)?;
    for (role, reference) in binding
        .definition
        .workflows
        .iter()
        .map(|(role, reference)| (role.as_str(), reference.as_str()))
        .chain(std::iter::once(("active evaluator", grade)))
    {
        let document = runtime
            .workflow(reference)
            .with_context(|| format!("{role} workflow {reference}"))?;
        if document
            .workflow
            .settings
            .io
            .result_map
            .as_ref()
            .is_none_or(|map| map.is_empty())
        {
            anyhow::bail!("{role} workflow must declare io.result_map before a run starts");
        }
        validate_authority(&document, &binding.requirements.authority)?;
    }
    if binding.definition.id == "software-security" {
        let temporary = tempfile::tempdir()?;
        if !binding
            .definition
            .assets
            .iter()
            .any(|asset| asset == "security.py")
        {
            anyhow::bail!("software-security must declare security.py in assets");
        }
        // Check the installed helper that will actually execute, never substitute
        // the binary's embedded template for user-selected source content.
        let script = runtime.asset("security.py")?;
        let input = temporary.path().join("input.json");
        fs::write(
            &input,
            serde_json::to_vec(&json!({
                "workspace": binding.context.root,
                "parameters": binding.requirements.parameters,
                "result_file": temporary.path().join("result.json"),
                "remaining_seconds": 60,
            }))?,
        )?;
        let mut command = tokio::process::Command::new("python3");
        command
            .arg("-c")
            .arg(script)
            .arg("preflight")
            .arg(input)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .context("security prerequisite checks exceeded 60 seconds")?
            .context("software-security requires Python 3; install it before running")?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
    }
    eprintln!("Preflight passed: workflows and declared prerequisites checked; no agent or candidate was started. Authentication and remote availability are checked by the configured agent when it runs.");
    Ok(())
}

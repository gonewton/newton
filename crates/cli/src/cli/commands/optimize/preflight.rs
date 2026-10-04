//! No agent dispatch, run creation, or candidate mutation during prerequisite checks.

use super::workflow::{validate_authority, validate_resource_metering};
use anyhow::{Context, Result};
use newton_types::optimization::BoundOptimizationDefinition;
use serde_json::{json, Value};
use std::{fs, time::Duration};

pub(super) async fn check(
    binding: &BoundOptimizationDefinition,
    runtime: &super::snapshot::Runtime,
) -> Result<()> {
    if !binding.definition.workflows.contains_key("propose") {
        anyhow::bail!("definition requires workflow role 'propose'");
    }
    if binding.definition.workflows.contains_key("promote") {
        anyhow::bail!(
            "promotion workflow is unsupported: the generic workflow host cannot independently verify the promoted target state or perform an atomic compare-and-swap; remove workflows.promote and retain the qualified candidate for an external, target-specific promotion boundary"
        );
    }
    let evaluate = super::workflow::evaluator_reference(&binding.requirements.requirements)?;
    for (role, reference) in binding
        .definition
        .workflows
        .iter()
        .map(|(role, reference)| (role.as_str(), reference.as_str()))
        .chain(std::iter::once(("active evaluator", evaluate)))
    {
        let document = runtime
            .workflow(reference)
            .with_context(|| format!("{role} workflow {reference}"))?;
        validate_resource_metering(&document)
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
        let registry = super::super::build_operator_registry_with_backend(
            binding.context.root.clone().into(),
            &document.workflow.settings,
            None,
            None,
            Some(0),
        );
        for task in document.workflow.tasks() {
            let operator = registry.get(&task.operator).with_context(|| {
                format!(
                    "{role} workflow {reference}, task '{}': operator '{}' is not registered",
                    task.id, task.operator
                )
            })?;
            let validation = if contains_expression(&task.params) {
                newton_core::workflow::schema_export::validate_authored_params(
                    &operator.params_schema(),
                    &task.params,
                )
                .and_then(|()| operator.validate_partial_params(&task.params))
            } else {
                operator.validate_params(&task.params)
            };
            validation.map_err(|error| {
                anyhow::anyhow!(
                    "{role} workflow {reference}, task '{}': {}: {}",
                    task.id,
                    error.code,
                    error.message
                )
            })?;
        }
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

// Match the canonical expression objects consumed by resolve_value.
fn contains_expression(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            (map.len() == 1 && map.get("$expr").is_some_and(Value::is_string))
                || map.values().any(contains_expression)
        }
        Value::Array(values) => values.iter().any(contains_expression),
        _ => false,
    }
}

use std::collections::HashMap;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::utils::insert_status_ids;
use super::GhOperator;

pub(super) fn validate_project_resolve_board(map: &Map<String, Value>) -> Result<(), AppError> {
    if map
        .get("owner")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "owner is required for project_resolve_board",
        ));
    }
    let project_number = map.get("project_number");
    match project_number {
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(Value::Number(_)) => {}
        _ => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "project_number is required for project_resolve_board",
            ));
        }
    }
    if let Some(arr) = map.get("required_option_names").and_then(Value::as_array) {
        if arr.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "required_option_names must be a non-empty array when set",
            ));
        }
        for v in arr {
            if v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "required_option_names must contain only non-empty strings",
                ));
            }
        }
    }
    Ok(())
}

impl GhOperator {
    pub(super) async fn execute_project_resolve_board(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let owner = map.get("owner").and_then(Value::as_str).unwrap();
        let project_number = map
            .get("project_number")
            .map(|v| {
                v.as_str()
                    .map_or_else(|| v.to_string(), std::string::ToString::to_string)
            })
            .unwrap();
        let field_name = map
            .get("field_name")
            .and_then(Value::as_str)
            .unwrap_or("Status");

        let view_output = self
            .runner
            .run(
                &[
                    "project",
                    "view",
                    &project_number,
                    "--owner",
                    owner,
                    "--format",
                    "json",
                ],
                workspace,
            )
            .await?;

        let view_json: Value = serde_json::from_str(&view_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project view JSON: {e}"),
            )
            .with_code("WFG-GH-001")
        })?;

        let project_id = view_json["id"].as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "project view missing id field",
            )
            .with_code("WFG-GH-001")
        })?;

        let fields_output = self
            .runner
            .run(
                &[
                    "project",
                    "field-list",
                    &project_number,
                    "--owner",
                    owner,
                    "--format",
                    "json",
                ],
                workspace,
            )
            .await?;

        let fields_json: Value = serde_json::from_str(&fields_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project field-list JSON: {e}"),
            )
            .with_code("WFG-GH-001")
        })?;

        let fields = fields_json["fields"].as_array().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "field-list missing fields array",
            )
            .with_code("WFG-GH-001")
        })?;

        let field = fields
            .iter()
            .find(|f| f["name"].as_str() == Some(field_name))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("field '{field_name}' not found"),
                )
                .with_code("WFG-GH-001")
            })?;

        let field_id = field["id"].as_str().ok_or_else(|| {
            AppError::new(ErrorCategory::ToolExecutionError, "field missing id")
                .with_code("WFG-GH-001")
        })?;

        let options = field["options"].as_array().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "field missing options array",
            )
            .with_code("WFG-GH-001")
        })?;

        let default_required = vec![
            "Ready".to_string(),
            "In progress".to_string(),
            "In review".to_string(),
            "Done".to_string(),
        ];
        let required_names: Vec<String> = map
            .get("required_option_names")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| default_required.clone());

        let mut found_options: Vec<String> = Vec::new();
        let mut options_map: HashMap<String, String> = HashMap::new();

        for opt in options {
            if let (Some(name), Some(id)) = (opt["name"].as_str(), opt["id"].as_str()) {
                found_options.push(name.to_string());
                options_map.insert(name.to_string(), id.to_string());
            }
        }

        for required in &required_names {
            if !options_map.contains_key(required) {
                return Err(AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!(
                        "required option '{required}' not found. Found options: {found_options:?}"
                    ),
                )
                .with_code("WFG-GH-001"));
            }
        }

        let mut out = serde_json::Map::new();
        out.insert("project_id".to_string(), json!(project_id));
        out.insert("field_id".to_string(), json!(field_id));
        out.insert(
            "options".to_string(),
            Value::Object(
                options_map
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ),
        );
        insert_status_ids(&options_map, &mut out);

        Ok(Value::Object(out))
    }
}

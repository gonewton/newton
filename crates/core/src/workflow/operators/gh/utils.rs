use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

pub const STATUS_KEY_MAP: &[(&str, &str)] = &[
    ("Idea", "idea_id"),
    ("Draft", "draft_id"),
    ("Backlog", "backlog_id"),
    ("Ready", "ready_id"),
    ("In progress", "in_progress_id"),
    ("In review", "in_review_id"),
    ("Done", "done_id"),
];

pub fn insert_status_ids(
    options_map: &std::collections::HashMap<String, String>,
    out: &mut serde_json::Map<String, Value>,
) {
    for (name, key) in STATUS_KEY_MAP {
        if let Some(id) = options_map.get(*name) {
            out.insert(key.to_string(), json!(id));
        }
    }
}

pub fn resolve_option_id(board: &Map<String, Value>, status: &str) -> Result<String, AppError> {
    if let Some(options) = board.get("options").and_then(Value::as_object) {
        if let Some(id) = options.get(status).and_then(Value::as_str) {
            return Ok(id.to_string());
        }
    }

    for (name, key) in STATUS_KEY_MAP {
        if status == *name {
            if let Some(id) = board.get(*key).and_then(Value::as_str) {
                return Ok(id.to_string());
            }
        }
    }

    Err(AppError::new(
        ErrorCategory::ValidationError,
        format!(
            "option id for status '{status}' not found in board (use board.options from project_resolve_board or pass single_select_option_id)",
        ),
    ))
}

pub fn get_pr_identifier(map: &Map<String, Value>) -> Result<String, AppError> {
    let pr = map.get("pr").ok_or_else(|| {
        AppError::new(ErrorCategory::ValidationError, "pr is required for pr_view")
    })?;

    match pr {
        Value::String(s) => {
            if s.contains("/pull/") {
                if let Some(num) = s.rsplit('/').next() {
                    return Ok(num.to_string());
                }
            }
            Ok(s.clone())
        }
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr must be a string or number",
        )),
    }
}

pub fn extract_pr_number(url: &str) -> Result<u64, AppError> {
    let parts: Vec<&str> = url.rsplit('/').collect();
    parts
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to extract PR number from: {url}"),
            )
            .with_code("WFG-GH-002")
        })
}

pub fn parse_pr_url(url: &str) -> Result<(String, u64), AppError> {
    if !url.starts_with("https://") {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr_url must use https scheme",
        )
        .with_code("WFG-GH-006"));
    }
    let without_scheme = &url["https://".len()..];
    let slash_pos = without_scheme.find('/').ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            "pr_url is not a valid GitHub pull request URL",
        )
        .with_code("WFG-GH-006")
    })?;
    let host = &without_scheme[..slash_pos];
    if !host.contains("github") {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr_url host must contain 'github'",
        )
        .with_code("WFG-GH-006"));
    }
    let path = &without_scheme[slash_pos..];
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 5 || parts[3] != "pull" {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr_url path must contain /pull/<number>",
        )
        .with_code("WFG-GH-006"));
    }
    let owner = parts[1];
    let repo = parts[2];
    let pr_number: u64 = parts[4].parse().map_err(|_| {
        AppError::new(
            ErrorCategory::ValidationError,
            "pr_url pull request number must be a positive integer",
        )
        .with_code("WFG-GH-006")
    })?;
    if pr_number < 1 {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr_url pull request number must be >= 1",
        )
        .with_code("WFG-GH-006"));
    }
    Ok((format!("{owner}/{repo}"), pr_number))
}

pub fn validate_repository_format(repo: &str) -> Result<(), AppError> {
    let slash_count = repo.chars().filter(|&c| c == '/').count();
    if slash_count != 1 {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "repository must be in owner/repo format",
        )
        .with_code("WFG-GH-007"));
    }
    let valid = repo
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || c == '/');
    if !valid || repo.starts_with('/') || repo.ends_with('/') {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "repository must match owner/repo format with valid characters",
        )
        .with_code("WFG-GH-007"));
    }
    Ok(())
}

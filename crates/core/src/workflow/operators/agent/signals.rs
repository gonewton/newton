use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use indexmap::IndexMap;
use regex::Regex;
use std::collections::HashMap;

/// Validate signal patterns in the config.
pub(super) fn validate_and_compile_signals(
    signals: &IndexMap<String, String>,
) -> Result<IndexMap<String, Regex>, AppError> {
    let mut compiled = IndexMap::new();
    for (name, pattern) in signals {
        if pattern.contains('\n') {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("signal '{name}' contains \\n; cross-line matching is not supported"),
            )
            .with_code("WFG-AGENT-004"));
        }
        let re = Regex::new(pattern).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("invalid regex in signal '{name}': {err}"),
            )
            .with_code("WFG-AGENT-004")
        })?;
        compiled.insert(name.clone(), re);
    }
    Ok(compiled)
}

/// Match a text line against compiled signals.
/// Returns (signal_name, captured_groups) for the first matching signal.
pub(super) fn match_signals(
    text: &str,
    signals: &IndexMap<String, Regex>,
) -> Option<(String, HashMap<String, String>)> {
    for (name, re) in signals {
        if let Some(caps) = re.captures(text) {
            let mut data = HashMap::new();
            for cn in re.capture_names().flatten() {
                if let Some(m) = caps.name(cn) {
                    data.insert(cn.to_string(), m.as_str().to_string());
                }
            }
            return Some((name.clone(), data));
        }
    }
    None
}

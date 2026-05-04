//! Unit coverage for `map_run_error` — ensures `aikit_sdk::RunError::QuotaExceeded`
//! is mapped to `WFG-AGENT-008` with the canonical context keys.
//!
//! Newton uses `run_agent_events` (which surfaces quota via `RunResult.quota_exceeded`,
//! not `Err`), so the `RunError::QuotaExceeded` arm is not exercised end-to-end. This
//! test covers AC-G6-b from the 251 spec.

use newton_core::workflow::operators::engine::map_run_error;

#[test]
fn sdk_quota_run_error_maps_to_agent_008() {
    let info = aikit_sdk::QuotaExceededInfo {
        agent_key: "claude".to_string(),
        category: aikit_sdk::QuotaCategory::Tokens,
        raw_message: "usage limit reached for tokens".to_string(),
    };
    let err = map_run_error(aikit_sdk::RunError::QuotaExceeded(info));

    assert_eq!(err.code, "WFG-AGENT-008", "must map to WFG-AGENT-008");
    assert_eq!(
        err.context.get("provider").map(|s| s.as_str()),
        Some("claude"),
        "provider context key must be set"
    );
    assert_eq!(
        err.context.get("quota_category").map(|s| s.as_str()),
        Some("tokens"),
        "quota_category context key must be set (lowercased)"
    );
    assert_eq!(
        err.context.get("raw_excerpt").map(|s| s.as_str()),
        Some("usage limit reached for tokens"),
        "raw_excerpt context key must be set"
    );
    // Minimal mapping: events_artifact / stderr_artifact are NOT set here — they are
    // attached post-hoc by execute_sdk_engine when it sees a WFG-AGENT-008 error.
    assert!(
        !err.context.contains_key("events_artifact"),
        "minimal mapping must NOT set events_artifact"
    );
}

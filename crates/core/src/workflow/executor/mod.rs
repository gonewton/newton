#![allow(clippy::result_large_err)]
use std::path::PathBuf;

use crate::core::error::AppError;
use crate::workflow::operator::OperatorRegistry;
use crate::workflow::schema::WorkflowDocument;
use uuid::Uuid;

mod child_runner;
mod diagnosis;
mod graph_handle;
mod helpers;
mod runtime;
mod types;

pub use crate::workflow::state::TaskStatus;
pub use child_runner::{resume_workflow, InProcessChildWorkflowRunner};
pub use diagnosis::TaskOutcome;
pub use graph_handle::GraphHandle;
pub use types::{ExecutionOverrides, ExecutionSummary};

use child_runner::build_workflow_runtime;
#[cfg(test)]
use helpers::shallow_merge_objects;

pub async fn execute_workflow(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<ExecutionSummary, AppError> {
    let runtime = build_workflow_runtime(
        document,
        workflow_path,
        registry,
        workspace_root,
        overrides,
        None,
    )?;
    runtime.run().await
}

pub fn spawn_workflow_execution(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<
    (
        Uuid,
        tokio::task::JoinHandle<Result<ExecutionSummary, AppError>>,
    ),
    AppError,
> {
    let runtime = build_workflow_runtime(
        document,
        workflow_path,
        registry,
        workspace_root,
        overrides,
        None,
    )?;
    let execution_id = runtime.workflow_execution.execution_id;
    let handle = tokio::spawn(async move { runtime.run().await });
    Ok((execution_id, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::state::{AppErrorSummary, TaskRunRecord, TaskStatus};
    use chrono::Utc;
    use diagnosis::{
        tail_truncate_utf8, write_task_failure_diagnosis, FailureDiagnosisInput,
        FAILURE_DIAGNOSIS_STREAM_CAP_BYTES,
    };
    use serde_json::{json, Value};

    #[test]
    fn shallow_merge_non_object_base_returns_err() {
        let err = shallow_merge_objects(&json!("string"), &json!({}))
            .expect_err("non-object base must error");
        assert_eq!(err.code, "WFG-NEST-005");
    }

    fn make_failed_record(output: Value, error_code: Option<&str>) -> TaskRunRecord {
        TaskRunRecord {
            status: TaskStatus::Failed,
            output,
            error_code: error_code.map(str::to_string),
            duration_ms: 0,
            run_seq: 1,
        }
    }

    fn make_failed_outcome(
        task_id: &str,
        record: TaskRunRecord,
        summary: Option<AppErrorSummary>,
    ) -> TaskOutcome {
        let now = Utc::now();
        TaskOutcome {
            task_id: task_id.to_string(),
            record,
            context_patch: None,
            failed: true,
            started_at: now,
            completed_at: now,
            error_summary: summary,
            resolved_params: json!({}),
        }
    }

    fn diagnose_to_string(input: FailureDiagnosisInput<'_>, verbose: bool) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_task_failure_diagnosis(&mut buf, input, verbose).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn tail_truncate_utf8_under_cap_is_lossless() {
        let s = "hello";
        let (slice, len, trunc) = tail_truncate_utf8(s, 100);
        assert_eq!(slice, "hello");
        assert_eq!(len, 5);
        assert!(!trunc);
    }

    #[test]
    fn tail_truncate_utf8_ascii_returns_tail_at_exact_boundary() {
        let s = "abcdefghij";
        let (slice, len, trunc) = tail_truncate_utf8(s, 4);
        assert_eq!(len, 10);
        assert!(trunc);
        assert_eq!(slice, "ghij");
    }

    #[test]
    fn tail_truncate_utf8_never_splits_multibyte_codepoint() {
        let s: String = "é".repeat(10);
        assert_eq!(s.len(), 20);
        let (slice, len, trunc) = tail_truncate_utf8(&s, 5);
        assert_eq!(len, 20);
        assert!(trunc);
        assert!(slice.len() <= 5);
        assert!(slice.len() % 2 == 0);
        for ch in slice.chars() {
            assert_eq!(ch, 'é');
        }
    }

    #[test]
    fn diagnosis_uses_error_summary_when_present() {
        let rec = make_failed_record(json!({}), Some("WFG-EXEC-001"));
        let summary = AppErrorSummary {
            code: "WFG-EXEC-007".to_string(),
            category: "ValidationError".to_string(),
            message: "summary message".to_string(),
            context: std::collections::HashMap::new(),
        };
        let outcome = make_failed_outcome("t1", rec, Some(summary));
        let out = diagnose_to_string(FailureDiagnosisInput::Outcome(&outcome), false);
        assert!(out.contains("--- task failed: t1 ---"), "got: {out}");
        assert!(out.contains("code=WFG-EXEC-007"), "got: {out}");
        assert!(out.contains("message=summary message"), "got: {out}");
    }

    #[test]
    fn diagnosis_falls_back_to_record_error_code_and_output_message() {
        let rec = make_failed_record(
            json!({ "error": { "message": "from output" } }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("code=WFG-CMD-001"), "got: {out}");
        assert!(out.contains("message=from output"), "got: {out}");
    }

    #[test]
    fn diagnosis_emits_message_unavailable_when_no_source() {
        let rec = make_failed_record(json!({}), None);
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("code=<unavailable>"), "got: {out}");
        assert!(out.contains("message=<unavailable>"), "got: {out}");
    }

    #[test]
    fn diagnosis_omits_empty_or_whitespace_streams() {
        let rec = make_failed_record(
            json!({
                "exit_code": 2,
                "stderr": "",
                "stdout": "   \n\n",
            }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("exit_code=2"), "got: {out}");
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
    }

    #[test]
    fn diagnosis_includes_command_streams_with_byte_headers() {
        let rec = make_failed_record(
            json!({
                "exit_code": 1,
                "stderr": "boom\n",
                "stdout": "ok-line",
            }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("exit_code=1"), "got: {out}");
        assert!(out.contains("--- stderr (5 bytes) ---"), "got: {out}");
        assert!(out.contains("boom"), "got: {out}");
        assert!(out.contains("--- stdout (7 bytes) ---"), "got: {out}");
        assert!(out.contains("ok-line"), "got: {out}");
    }

    #[test]
    fn diagnosis_truncates_oversized_stream_with_marker() {
        let big = "x".repeat(FAILURE_DIAGNOSIS_STREAM_CAP_BYTES + 100);
        let rec = make_failed_record(
            json!({ "exit_code": 1, "stderr": big.clone() }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(
            out.contains(&format!(
                "truncated to {} bytes",
                FAILURE_DIAGNOSIS_STREAM_CAP_BYTES
            )),
            "got: {out}"
        );
        assert!(
            out.contains(&format!("({} bytes,", big.len())),
            "got: {out}"
        );
    }

    #[test]
    fn diagnosis_for_agent_output_prints_artifact_paths() {
        let rec = make_failed_record(
            json!({
                "stdout_artifact": "/tmp/agent.stdout",
                "stderr_artifact": "/tmp/agent.stderr",
            }),
            Some("WFG-AGENT-005"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "agent_t",
                record: &rec,
            },
            false,
        );
        assert!(
            out.contains("stderr artifact: /tmp/agent.stderr"),
            "got: {out}"
        );
        assert!(
            out.contains("stdout artifact: /tmp/agent.stdout"),
            "got: {out}"
        );
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
    }

    #[test]
    fn diagnosis_with_verbose_suppresses_stream_bodies_only() {
        let rec = make_failed_record(
            json!({
                "exit_code": 1,
                "stderr": "boom\n",
                "stdout": "ok\n",
                "stderr_artifact": "/a/err",
                "stdout_artifact": "/a/out",
            }),
            Some("WFG-CMD-001"),
        );
        let outcome = make_failed_outcome("tx", rec, None);
        let out = diagnose_to_string(FailureDiagnosisInput::Outcome(&outcome), true);
        assert!(out.contains("--- task failed: tx ---"), "got: {out}");
        assert!(out.contains("exit_code=1"), "got: {out}");
        assert!(out.contains("stderr artifact: /a/err"), "got: {out}");
        assert!(out.contains("stdout artifact: /a/out"), "got: {out}");
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
        assert!(!out.contains("boom"), "got: {out}");
    }
}

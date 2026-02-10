use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// Structured representation of messages coming from ailoop.
#[derive(Debug, Clone)]
pub struct MonitorMessage {
    /// Unique identifier of the message.
    pub id: Uuid,
    /// Full channel name (project/branch).
    pub channel: String,
    /// Parsed kind of message content.
    pub kind: MessageKind,
    /// Primary text displayed in stream tiles/lists.
    pub summary: String,
    /// Optional longer text (e.g. question text).
    pub text: Option<String>,
    /// Optional contextual details (e.g. authorization context or response).
    pub detail: Option<String>,
    /// Timestamp indicating when the message was emitted.
    pub timestamp: DateTime<Utc>,
    /// References another message when this is a response.
    pub correlation_id: Option<Uuid>,
    /// Timeout in seconds for blocking content (question/authorization).
    pub timeout_seconds: Option<u64>,
    /// Choices attached to a question.
    pub choices: Vec<String>,
    /// Response metadata for response messages.
    pub response_type: Option<String>,
}

impl MonitorMessage {
    /// Whether this message should appear in the queue (blocking).
    pub fn is_blocking(&self) -> bool {
        matches!(
            self.kind,
            MessageKind::Question | MessageKind::Authorization
        )
    }

    /// Pretty string describing the timeout threshold (if any).
    pub fn timeout_description(&self) -> Option<String> {
        self.timeout_seconds.map(|seconds| format!("{}s", seconds))
    }
}

impl<'de> Deserialize<'de> for MonitorMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_message(value).map_err(serde::de::Error::custom)
    }
}

fn parse_message(value: Value) -> Result<MonitorMessage, anyhow::Error> {
    use anyhow::Context;

    let id =
        string_field(&value, &["id", "message_id"]).ok_or_else(|| anyhow::anyhow!("missing id"))?;
    let id = Uuid::parse_str(&id).context("invalid message id")?;

    let channel = string_field(&value, &["channel", "channel_id"])
        .ok_or_else(|| anyhow::anyhow!("missing channel"))?;

    let correlation_id = string_field(&value, &["correlation_id", "correlationId"])
        .and_then(|val| Uuid::parse_str(&val).ok());

    let timestamp = timestamp_field(&value).unwrap_or_else(Utc::now);

    let content = value
        .get("content")
        .cloned()
        .unwrap_or_else(|| value.clone());

    let type_hint = string_field(&content, &["type"])
        .or_else(|| string_field(&value, &["type"]))
        .unwrap_or_else(|| "unknown".to_string());

    let kind = MessageKind::from_type(&type_hint);
    let text = string_field(&content, &["text", "message", "body"]);
    let detail = string_field(&content, &["context", "action", "status"]);
    let timeout_seconds = numeric_field(&content, &["timeout_seconds", "timeoutSeconds"]);
    let choices = array_field_strings(&content, &["choices"]);
    let response_type = string_field(&content, &["response_type", "responseType"]);

    let summary = build_summary(
        &kind,
        text.as_deref(),
        detail.as_deref(),
        &choices,
        response_type.as_deref(),
    );

    Ok(MonitorMessage {
        id,
        channel,
        kind,
        summary,
        text,
        detail,
        timestamp,
        correlation_id,
        timeout_seconds,
        choices,
        response_type,
    })
}

fn build_summary(
    kind: &MessageKind,
    text: Option<&str>,
    detail: Option<&str>,
    choices: &[String],
    response_type: Option<&str>,
) -> String {
    match kind {
        MessageKind::Question => {
            let question = text.unwrap_or("Question");
            if choices.is_empty() {
                format!("Question: {}", question)
            } else {
                let choice_list = choices.join(", ");
                format!("Question: {} [{}]", question, choice_list)
            }
        }
        MessageKind::Authorization => {
            let action = detail.unwrap_or("Authorization required");
            format!("Authorization: {}", action)
        }
        MessageKind::Notification => {
            format!("Notification: {}", text.unwrap_or("Notification"))
        }
        MessageKind::Response => {
            let target = response_type.unwrap_or("response");
            format!("Response: {}", target)
        }
        MessageKind::Navigate => {
            format!("Navigate: {}", text.unwrap_or("Navigate"))
        }
        MessageKind::WorkflowProgress => {
            format!("Workflow progress: {}", text.unwrap_or("In-flight"))
        }
        MessageKind::WorkflowCompleted => {
            format!("Workflow completed: {}", text.unwrap_or("Done"))
        }
        MessageKind::Stdout => {
            format!("stdout: {}", text.unwrap_or(""))
        }
        MessageKind::Stderr => {
            format!("stderr: {}", text.unwrap_or(""))
        }
        MessageKind::TaskCreate => {
            format!("Task created: {}", text.unwrap_or("Task"))
        }
        MessageKind::TaskUpdate => {
            format!("Task update: {}", text.unwrap_or("Task updated"))
        }
        MessageKind::TaskDependencyAdd => {
            format!("Dependency added: {}", text.unwrap_or("Dependency change"))
        }
        MessageKind::TaskDependencyRemove => {
            format!(
                "Dependency removed: {}",
                text.unwrap_or("Dependency change")
            )
        }
        MessageKind::Unknown(label) => label.clone(),
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
}

fn timestamp_field(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts) = value.get("timestamp").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_num) = value.get("timestamp").and_then(|v| v.as_i64()) {
        if let Some(dt) = Utc.timestamp_opt(ts_num, 0).single() {
            return Some(dt);
        }
    }
    None
}

fn numeric_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|v| v.as_u64())
}

fn array_field_strings(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|item| match item {
            Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|entry| {
                        entry.as_str().map(|s| s.trim().to_string()).or_else(|| {
                            entry
                                .get("text")
                                .and_then(|x| x.as_str())
                                .map(|s| s.trim().to_string())
                        })
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// Enumeration of message content variants emitted by the ailoop server.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageKind {
    Question,
    Authorization,
    Notification,
    Response,
    Navigate,
    WorkflowProgress,
    WorkflowCompleted,
    Stdout,
    Stderr,
    TaskCreate,
    TaskUpdate,
    TaskDependencyAdd,
    TaskDependencyRemove,
    Unknown(String),
}

impl MessageKind {
    fn from_type(type_name: &str) -> Self {
        match type_name {
            "question" => MessageKind::Question,
            "authorization" => MessageKind::Authorization,
            "notification" => MessageKind::Notification,
            "response" => MessageKind::Response,
            "navigate" => MessageKind::Navigate,
            "workflow_progress" | "workflow_progress_update" => MessageKind::WorkflowProgress,
            "workflow_completed" => MessageKind::WorkflowCompleted,
            "stdout" => MessageKind::Stdout,
            "stderr" => MessageKind::Stderr,
            "task_create" => MessageKind::TaskCreate,
            "task_update" => MessageKind::TaskUpdate,
            "task_dependency_add" => MessageKind::TaskDependencyAdd,
            "task_dependency_remove" => MessageKind::TaskDependencyRemove,
            other => MessageKind::Unknown(other.to_string()),
        }
    }
}

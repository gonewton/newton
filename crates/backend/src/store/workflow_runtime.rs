use super::helpers::*;
use super::rows::*;
use crate::err_internal;
use crate::err_not_found;
use newton_types::ApiError;
use uuid::Uuid;

impl super::SqliteBackendStore {
    pub(super) async fn get_workflow_instance(
        &self,
        instance_id: &str,
    ) -> Result<newton_types::WorkflowInstance, ApiError> {
        let row: Option<WorkflowInstanceRow> = sqlx::query_as::<_, WorkflowInstanceRow>(
            "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance WHERE instanceId = ?"
        )
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Workflow instance not found"))?;
        let nodes = self.list_node_states_for_instance(instance_id).await?;
        wi_row_to_instance(row, nodes)
    }

    pub(super) async fn list_workflow_instances(
        &self,
        status: Option<newton_types::WorkflowStatus>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<newton_types::WorkflowInstance>, ApiError> {
        let rows: Vec<WorkflowInstanceRow> = match &status {
            Some(s) => {
                sqlx::query_as::<_, WorkflowInstanceRow>(
                    "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance WHERE status = ? ORDER BY startedAt DESC LIMIT ? OFFSET ?"
                )
                .bind(workflow_status_str(s))
                .bind(limit.unwrap_or(100) as i64)
                .bind(offset.unwrap_or(0) as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
            }
            None => {
                sqlx::query_as::<_, WorkflowInstanceRow>(
                    "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance ORDER BY startedAt DESC LIMIT ? OFFSET ?"
                )
                .bind(limit.unwrap_or(100) as i64)
                .bind(offset.unwrap_or(0) as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
            }
        };

        let mut instances = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.instance_id.clone();
            let nodes = self.list_node_states_for_instance(&id).await?;
            instances.push(wi_row_to_instance(row, nodes)?);
        }
        Ok(instances)
    }

    pub(super) async fn upsert_workflow_instance(
        &self,
        instance: &newton_types::WorkflowInstance,
    ) -> Result<(), ApiError> {
        let now = chrono::Utc::now().to_rfc3339();
        let definition_json = instance
            .definition
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| err_internal(&format!("definition serialize: {e}")))?;

        sqlx::query(
            "INSERT INTO WorkflowInstance (instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition, createdAt, updatedAt)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(instanceId) DO UPDATE SET
               workflowId = excluded.workflowId,
               status = excluded.status,
               linkedPlanId = excluded.linkedPlanId,
               startedAt = excluded.startedAt,
               endedAt = excluded.endedAt,
               definition = excluded.definition,
               updatedAt = excluded.updatedAt"
        )
        .bind(&instance.instance_id)
        .bind(&instance.workflow_id)
        .bind(workflow_status_str(&instance.status))
        .bind(&instance.linked_plan_id)
        .bind(instance.started_at.to_rfc3339())
        .bind(instance.ended_at.map(|dt| dt.to_rfc3339()))
        .bind(definition_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert error: {e}")))?;

        Ok(())
    }

    pub(super) async fn delete_workflow_instance(&self, instance_id: &str) -> Result<(), ApiError> {
        let affected = sqlx::query("DELETE FROM WorkflowInstance WHERE instanceId = ?")
            .bind(instance_id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Workflow instance not found"));
        }
        Ok(())
    }

    pub(super) async fn get_node_state(
        &self,
        instance_id: &str,
        node_id: &str,
    ) -> Result<newton_types::NodeState, ApiError> {
        let row: Option<NodeStateRow> = sqlx::query_as::<_, NodeStateRow>(
            "SELECT instanceId, nodeId, status, startedAt, endedAt, operatorType FROM NodeState WHERE instanceId = ? AND nodeId = ?"
        )
        .bind(instance_id)
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Node state not found"))?;
        row_to_node_state(row)
    }

    pub(super) async fn list_node_states_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::NodeState>, ApiError> {
        let rows: Vec<NodeStateRow> = sqlx::query_as::<_, NodeStateRow>(
            "SELECT instanceId, nodeId, status, startedAt, endedAt, operatorType FROM NodeState WHERE instanceId = ? ORDER BY rowid ASC"
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter().map(row_to_node_state).collect()
    }

    pub(super) async fn upsert_node_state(
        &self,
        instance_id: &str,
        node: &newton_types::NodeState,
    ) -> Result<(), ApiError> {
        let id = format!("{}-{}", instance_id, node.node_id);
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO NodeState (id, instanceId, nodeId, status, startedAt, endedAt, operatorType)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(instanceId, nodeId) DO UPDATE SET
               status = excluded.status,
               startedAt = excluded.startedAt,
               endedAt = excluded.endedAt,
               operatorType = excluded.operatorType"
        )
        .bind(&id)
        .bind(instance_id)
        .bind(&node.node_id)
        .bind(node_status_str(&node.status))
        .bind(node.started_at.map(|dt| dt.to_rfc3339()))
        .bind(node.ended_at.map(|dt| dt.to_rfc3339()))
        .bind(&node.operator_type)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert node state error: {e}")))?;

        let _ = now;
        Ok(())
    }

    pub(super) async fn update_workflow_status(
        &self,
        instance_id: &str,
        status: newton_types::WorkflowStatus,
        ended_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ApiError> {
        let now = Self::now_iso();
        let affected = sqlx::query(
            "UPDATE WorkflowInstance SET status = ?, endedAt = ?, updatedAt = ? WHERE instanceId = ?"
        )
        .bind(workflow_status_str(&status))
        .bind(ended_at.to_rfc3339())
        .bind(&now)
        .bind(instance_id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update_workflow_status error: {e}")))?;

        if affected.rows_affected() == 0 {
            return Err(err_not_found("Workflow instance not found"));
        }
        Ok(())
    }

    pub(super) async fn get_hil_event(
        &self,
        event_id: &str,
    ) -> Result<newton_types::HilEvent, ApiError> {
        let row: Option<HilEventRow> = sqlx::query_as::<_, HilEventRow>(
            "SELECT eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp FROM HilEvent WHERE eventId = ?"
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("HIL event not found"))?;
        row_to_hil_event(row)
    }

    pub(super) async fn list_hil_events_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::HilEvent>, ApiError> {
        let rows: Vec<HilEventRow> = sqlx::query_as::<_, HilEventRow>(
            "SELECT eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp FROM HilEvent WHERE instanceId = ? ORDER BY rowid ASC"
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter().map(row_to_hil_event).collect()
    }

    pub(super) async fn list_hil_instances(&self) -> Result<Vec<String>, ApiError> {
        let rows: Vec<InstanceIdRow> = sqlx::query_as::<_, InstanceIdRow>(
            "SELECT DISTINCT instanceId FROM HilEvent ORDER BY instanceId ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows.into_iter().map(|r| r.instance_id).collect())
    }

    pub(super) async fn insert_hil_event(
        &self,
        event: &newton_types::HilEvent,
    ) -> Result<(), ApiError> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let choices_json = serde_json::to_string(&event.choices)
            .map_err(|e| err_internal(&format!("choices serialize: {e}")))?;

        sqlx::query(
            "INSERT INTO HilEvent (id, eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp, createdAt, updatedAt)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(&event.event_id)
        .bind(&event.instance_id)
        .bind(&event.node_id)
        .bind(&event.channel)
        .bind(hil_event_type_str(&event.event_type))
        .bind(&event.question)
        .bind(&choices_json)
        .bind(event.timeout_seconds.map(|v| v as i64))
        .bind(event.correlation_id.map(|u| u.to_string()))
        .bind(hil_status_str(&event.status))
        .bind(event.timestamp.to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert HIL event error: {e}")))?;

        Ok(())
    }

    pub(super) async fn update_hil_event_status(
        &self,
        event_id: &str,
        status: newton_types::HilStatus,
    ) -> Result<newton_types::HilEvent, ApiError> {
        let now = chrono::Utc::now().to_rfc3339();
        let affected =
            sqlx::query("UPDATE HilEvent SET status = ?, updatedAt = ? WHERE eventId = ?")
                .bind(hil_status_str(&status))
                .bind(&now)
                .bind(event_id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update error: {e}")))?;

        if affected.rows_affected() == 0 {
            return Err(err_not_found("HIL event not found"));
        }
        self.get_hil_event(event_id).await
    }

    pub(super) async fn append_log_line(
        &self,
        instance_id: &str,
        node_id: &str,
        line: &newton_types::LogLine,
    ) -> Result<(), ApiError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        sqlx::query(
            "INSERT INTO WorkflowLog (instanceId, nodeId, seq, ts, level, message)
             VALUES (?, ?, COALESCE((SELECT MAX(seq) FROM WorkflowLog WHERE instanceId = ? AND nodeId = ?), 0) + 1, ?, ?, ?)"
        )
        .bind(instance_id)
        .bind(node_id)
        .bind(instance_id)
        .bind(node_id)
        .bind(line.timestamp.to_rfc3339())
        .bind(&line.level)
        .bind(&line.message)
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("append log line error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit tx error: {e}")))?;

        Ok(())
    }

    pub(super) async fn list_log_lines(
        &self,
        instance_id: &str,
        node_id: &str,
        since_seq: i64,
    ) -> Result<Vec<newton_types::LogLine>, ApiError> {
        let rows: Vec<WorkflowLogRow> = sqlx::query_as::<_, WorkflowLogRow>(
            "SELECT seq, instanceId, nodeId, ts, level, message FROM WorkflowLog WHERE instanceId = ? AND nodeId = ? AND seq > ? ORDER BY seq ASC"
        )
        .bind(instance_id)
        .bind(node_id)
        .bind(since_seq)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter()
            .map(|r| {
                Ok(newton_types::LogLine {
                    instance_id: r.instance_id,
                    node_id: r.node_id,
                    level: r.level,
                    message: r.message,
                    timestamp: parse_dt(&r.ts)?,
                })
            })
            .collect()
    }
}

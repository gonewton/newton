PRAGMA journal_mode=WAL;

-- WorkflowInstance mirrors newton-ui/backend/prisma/schema.prisma model WorkflowInstance.
CREATE TABLE IF NOT EXISTS WorkflowInstance (
  instanceId   TEXT PRIMARY KEY,
  workflowId   TEXT NOT NULL,
  status       TEXT NOT NULL,
  linkedPlanId TEXT NULL,
  startedAt    TEXT NOT NULL,
  endedAt      TEXT NULL,
  definition   TEXT NULL,  -- JSON-serialised serde_json::Value
  createdAt    TEXT NOT NULL,
  updatedAt    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflowinstance_status      ON WorkflowInstance(status);
CREATE INDEX IF NOT EXISTS idx_workflowinstance_linkedPlanId ON WorkflowInstance(linkedPlanId);

-- NodeState: operator_type is a Rust-only extension (strict superset of Prisma schema).
CREATE TABLE IF NOT EXISTS NodeState (
  id           TEXT PRIMARY KEY,
  instanceId   TEXT NOT NULL,
  nodeId       TEXT NOT NULL,
  status       TEXT NOT NULL,
  startedAt    TEXT NULL,
  endedAt      TEXT NULL,
  operatorType TEXT NULL,  -- Rust extension: newton_types::NodeState.operator_type
  FOREIGN KEY(instanceId) REFERENCES WorkflowInstance(instanceId) ON DELETE CASCADE,
  UNIQUE(instanceId, nodeId)
);
CREATE INDEX IF NOT EXISTS idx_nodestate_instanceId ON NodeState(instanceId);

CREATE TABLE IF NOT EXISTS HilEvent (
  id             TEXT PRIMARY KEY,
  eventId        TEXT NOT NULL UNIQUE,
  instanceId     TEXT NOT NULL,
  nodeId         TEXT NULL,
  channel        TEXT NOT NULL,
  eventType      TEXT NOT NULL,
  question       TEXT NOT NULL,
  choices        TEXT NOT NULL DEFAULT '[]',  -- JSON-serialised Vec<String>
  timeoutSeconds INTEGER NULL,
  correlationId  TEXT NULL,
  status         TEXT NOT NULL,
  timestamp      TEXT NOT NULL,
  createdAt      TEXT NOT NULL,
  updatedAt      TEXT NOT NULL,
  FOREIGN KEY(instanceId) REFERENCES WorkflowInstance(instanceId) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_hilevent_instanceId ON HilEvent(instanceId);
CREATE INDEX IF NOT EXISTS idx_hilevent_status     ON HilEvent(status);

-- RepoDependency is excluded from this migration. It is computed at query time from the
-- existing ModuleDependency+Module tables (001_init.sql) and is not a stored entity.

-- Append-only log table. seq is computed as MAX(seq)+1 per (instanceId, nodeId) pair.
-- Efficient tailing: WHERE instanceId=? AND nodeId=? AND seq > ?
CREATE TABLE IF NOT EXISTS WorkflowLog (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  instanceId TEXT NOT NULL,
  nodeId     TEXT NOT NULL,
  seq        INTEGER NOT NULL,
  ts         TEXT NOT NULL,  -- ISO-8601 timestamp
  level      TEXT NOT NULL,
  message    TEXT NOT NULL,
  FOREIGN KEY(instanceId) REFERENCES WorkflowInstance(instanceId) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_workflowlog_instance_node_seq
  ON WorkflowLog(instanceId, nodeId, seq);

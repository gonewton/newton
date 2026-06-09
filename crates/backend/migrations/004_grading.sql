PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS Request;
DROP TABLE IF EXISTS Opportunity;

CREATE TABLE IF NOT EXISTS Finding (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  origin TEXT NOT NULL DEFAULT 'system',
  componentId TEXT NULL,
  module TEXT NULL,
  repoId TEXT NULL,
  kpiId TEXT NULL,
  dimension TEXT NOT NULL,
  location TEXT NULL,
  fingerprint TEXT NOT NULL,
  title TEXT NOT NULL,
  whyItMatters TEXT NOT NULL DEFAULT '',
  recommendedAction TEXT NOT NULL DEFAULT '',
  severity TEXT NOT NULL DEFAULT 'medium',
  risk TEXT NOT NULL DEFAULT 'medium',
  confidence REAL NULL,
  evidence TEXT NULL,
  expectedValue REAL NULL,
  effort TEXT NULL,
  status TEXT NOT NULL DEFAULT 'awaiting_triage',
  lastSeenAt TEXT NOT NULL,
  dependsOn TEXT NOT NULL DEFAULT '[]',
  blocks TEXT NOT NULL DEFAULT '[]',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id),
  FOREIGN KEY(kpiId) REFERENCES KPI(id)
);
CREATE INDEX IF NOT EXISTS idx_finding_status ON Finding(status);
CREATE INDEX IF NOT EXISTS idx_finding_componentId ON Finding(componentId);
CREATE INDEX IF NOT EXISTS idx_finding_repoId ON Finding(repoId);
CREATE INDEX IF NOT EXISTS idx_finding_fingerprint ON Finding(fingerprint);

CREATE TABLE IF NOT EXISTS ChangeRequest (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  body TEXT NULL,
  origin TEXT NOT NULL DEFAULT 'system',
  author TEXT NULL,
  componentId TEXT NULL,
  repoId TEXT NULL,
  status TEXT NOT NULL DEFAULT 'proposed',
  findingIds TEXT NOT NULL DEFAULT '[]',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id)
);
CREATE INDEX IF NOT EXISTS idx_changerequest_status ON ChangeRequest(status);

CREATE TABLE IF NOT EXISTS Plan_new (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  componentId TEXT NULL,
  repoId TEXT NULL,
  status TEXT NOT NULL,
  linkedChangeRequestId TEXT NULL,
  confidence INTEGER NOT NULL,
  risk TEXT NOT NULL,
  expectedValue TEXT NULL,
  agentGenerated INTEGER NOT NULL DEFAULT 0,
  waitingSince TEXT NULL,
  expectedDelta TEXT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id),
  FOREIGN KEY(linkedChangeRequestId) REFERENCES ChangeRequest(id)
);

INSERT OR IGNORE INTO Plan_new (
  id, title, componentId, repoId, status, linkedChangeRequestId,
  confidence, risk, expectedValue, agentGenerated, waitingSince,
  expectedDelta, createdAt, updatedAt
)
SELECT
  id, title, componentId, repoId, status, NULL,
  confidence, risk, expectedValue, agentGenerated, waitingSince,
  expectedDelta, createdAt, updatedAt
FROM Plan;

DROP TABLE Plan;
ALTER TABLE Plan_new RENAME TO Plan;

CREATE INDEX IF NOT EXISTS idx_plan_status ON Plan(status);
CREATE INDEX IF NOT EXISTS idx_plan_componentId ON Plan(componentId);

PRAGMA foreign_keys = ON;

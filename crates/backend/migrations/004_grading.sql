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

-- NOTE: The legacy Plan rebuild (linkedRequestId -> linkedChangeRequestId,
-- re-pointing the FK from Request to ChangeRequest) was previously performed
-- here with an `INSERT ... SELECT ..., NULL, ...` into a Plan_new table. This
-- file is re-executed on EVERY store open, so that rebuild wiped every plan's
-- linkedChangeRequestId (and other columns) on each open. The rebuild now lives
-- in the guarded Rust migration `upgrade_plan_change_request_link`, which only
-- runs when the legacy `linkedRequestId` column is still present.

PRAGMA foreign_keys = ON;

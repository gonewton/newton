-- 006: OptimizeRun + OptimizeCycle tables; blocked_by_plan_id on Finding

PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS OptimizeRun (
  id            TEXT PRIMARY KEY,
  projectId     TEXT NOT NULL,
  scope         TEXT NOT NULL DEFAULT 'repo',
  scopeId       TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'running',
  cycle         INTEGER NOT NULL DEFAULT 0,
  maxCycles     INTEGER NOT NULL DEFAULT 8,
  graders       TEXT NOT NULL DEFAULT '[]',
  latestGrades  TEXT NOT NULL DEFAULT '{}',
  openFindings  INTEGER NOT NULL DEFAULT 0,
  blockedFindings INTEGER NOT NULL DEFAULT 0,
  outcomeReason TEXT NULL,
  startedAt     TEXT NOT NULL,
  updatedAt     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS OptimizeCycle (
  id                TEXT PRIMARY KEY,
  runId             TEXT NOT NULL,
  cycle             INTEGER NOT NULL,
  grades            TEXT NOT NULL DEFAULT '{}',
  gradeMin          REAL NULL,
  decision          TEXT NOT NULL DEFAULT 'none',
  changeRequestId   TEXT NULL,
  planId            TEXT NULL,
  executionId       TEXT NULL,
  developStatus     TEXT NULL,
  openFindings      INTEGER NOT NULL DEFAULT 0,
  resolvedThisCycle INTEGER NOT NULL DEFAULT 0,
  createdAt         TEXT NOT NULL,
  FOREIGN KEY(runId) REFERENCES OptimizeRun(id)
);

PRAGMA foreign_keys = ON;

-- Grade: per-run, per-dimension evidence (append-only).
--
-- Note: This file is executed on every store open (not a one-time migration table),
-- so it must be idempotent and must not destroy data already in the new schema.
-- Legacy schema upgrades (if detected) are handled in Rust during store initialization
-- and may rebuild the table (dropping legacy rows that cannot be migrated).
CREATE TABLE IF NOT EXISTS Grade (
  id          TEXT PRIMARY KEY,
  runId       TEXT NOT NULL,
  kpiId       TEXT NULL,
  dimension   TEXT NOT NULL,
  score       REAL NOT NULL CHECK(score >= 0 AND score <= 100),
  evidence    TEXT NULL,
  evaluatedAt TEXT NOT NULL,
  ingestedAt  TEXT NOT NULL,
  UNIQUE(runId, dimension),
  FOREIGN KEY(runId) REFERENCES EvalRun(id) ON DELETE CASCADE,
  FOREIGN KEY(kpiId) REFERENCES KPI(id)
);
CREATE INDEX IF NOT EXISTS idx_grade_runId ON Grade(runId);
CREATE INDEX IF NOT EXISTS idx_grade_kpiId ON Grade(kpiId);

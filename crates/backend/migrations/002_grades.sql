CREATE TABLE IF NOT EXISTS Grade (
  id          TEXT PRIMARY KEY,
  scope       TEXT NOT NULL CHECK(scope IN ('component', 'module')),
  scopeId     TEXT NOT NULL,
  indicator   TEXT NOT NULL,
  score       REAL NOT NULL CHECK(score >= 0 AND score <= 100),
  metrics     TEXT,
  detailsUrl  TEXT,
  rawOutput   TEXT,
  evaluatedAt TEXT NOT NULL,
  ingestedAt  TEXT NOT NULL,
  UNIQUE(scope, scopeId, indicator)
);
CREATE INDEX IF NOT EXISTS idx_grade_scope_scopeId ON Grade(scope, scopeId);
CREATE INDEX IF NOT EXISTS idx_grade_indicator ON Grade(indicator);

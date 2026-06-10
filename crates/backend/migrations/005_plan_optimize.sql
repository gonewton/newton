-- 005: extend Plan for the optimize loop; add risk/confidence to ChangeRequest

PRAGMA foreign_keys = OFF;

-- Plan: add body, executionId, attempts, lastError, module
ALTER TABLE Plan ADD COLUMN body TEXT NULL;
ALTER TABLE Plan ADD COLUMN executionId TEXT NULL;
ALTER TABLE Plan ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE Plan ADD COLUMN lastError TEXT NULL;
ALTER TABLE Plan ADD COLUMN module TEXT NULL;

-- ChangeRequest: add risk/confidence so Plan can copy them
ALTER TABLE ChangeRequest ADD COLUMN risk TEXT NOT NULL DEFAULT 'medium';
ALTER TABLE ChangeRequest ADD COLUMN confidence REAL NULL;

PRAGMA foreign_keys = ON;

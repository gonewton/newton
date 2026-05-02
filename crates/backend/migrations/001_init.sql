PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS Product (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS Component (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  domain TEXT NOT NULL,
  repos INTEGER NOT NULL,
  modules INTEGER NOT NULL,
  health INTEGER NOT NULL,
  trend INTEGER NOT NULL,
  owner TEXT NOT NULL,
  criticality TEXT NOT NULL,
  autonomy TEXT NOT NULL,
  openPlans INTEGER NOT NULL DEFAULT 0,
  openRequests INTEGER NOT NULL DEFAULT 0,
  lastEval TEXT NOT NULL,
  productId TEXT NOT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(productId) REFERENCES Product(id)
);
CREATE INDEX IF NOT EXISTS idx_component_productId ON Component(productId);

CREATE TABLE IF NOT EXISTS Repo (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  componentId TEXT NOT NULL,
  owner TEXT NOT NULL,
  criticality TEXT NOT NULL,
  autonomy TEXT NOT NULL,
  qualityScore INTEGER NOT NULL,
  regressions INTEGER NOT NULL DEFAULT 0,
  openPlans INTEGER NOT NULL DEFAULT 0,
  execStatus TEXT NOT NULL,
  lastEval TEXT NOT NULL,
  coverage INTEGER NOT NULL,
  secScore INTEGER NOT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id)
);
CREATE INDEX IF NOT EXISTS idx_repo_componentId ON Repo(componentId);

CREATE TABLE IF NOT EXISTS Module (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  repoId TEXT NOT NULL,
  FOREIGN KEY(repoId) REFERENCES Repo(id)
);
CREATE INDEX IF NOT EXISTS idx_module_repoId ON Module(repoId);

CREATE TABLE IF NOT EXISTS ModuleDependency (
  id TEXT PRIMARY KEY,
  fromModuleId TEXT NOT NULL,
  toModuleId TEXT NOT NULL,
  type TEXT NOT NULL,
  label TEXT NOT NULL,
  FOREIGN KEY(fromModuleId) REFERENCES Module(id),
  FOREIGN KEY(toModuleId) REFERENCES Module(id),
  UNIQUE(fromModuleId, toModuleId)
);
CREATE INDEX IF NOT EXISTS idx_moduledep_from ON ModuleDependency(fromModuleId);
CREATE INDEX IF NOT EXISTS idx_moduledep_to ON ModuleDependency(toModuleId);

CREATE TABLE IF NOT EXISTS PendingApproval (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  type TEXT NOT NULL,
  componentId TEXT NULL,
  repoName TEXT NULL,
  risk TEXT NOT NULL,
  expectedValue TEXT NOT NULL,
  waitingSince TEXT NOT NULL,
  reviewer TEXT NOT NULL,
  status TEXT NOT NULL,
  confidence INTEGER NOT NULL,
  agentGenerated INTEGER NOT NULL DEFAULT 0,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id)
);
CREATE INDEX IF NOT EXISTS idx_pendingapproval_status ON PendingApproval(status);

CREATE TABLE IF NOT EXISTS Opportunity (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  origin TEXT NOT NULL,
  componentId TEXT NULL,
  module TEXT NULL,
  repoId TEXT NULL,
  indicator TEXT NULL,
  confidence REAL NULL,
  risk TEXT NOT NULL,
  expectedValue REAL NOT NULL,
  effort TEXT NULL,
  status TEXT NOT NULL,
  age TEXT NULL,
  rationale TEXT NULL,
  dependsOn TEXT NOT NULL DEFAULT '[]',
  blocks TEXT NOT NULL DEFAULT '[]',
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id)
);
CREATE INDEX IF NOT EXISTS idx_opportunity_status ON Opportunity(status);
CREATE INDEX IF NOT EXISTS idx_opportunity_componentId ON Opportunity(componentId);

CREATE TABLE IF NOT EXISTS Request (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  description TEXT NULL,
  componentId TEXT NULL,
  repoId TEXT NULL,
  requestedBy TEXT NOT NULL,
  status TEXT NOT NULL,
  linkedOpportunityId TEXT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL,
  FOREIGN KEY(componentId) REFERENCES Component(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id)
);
CREATE INDEX IF NOT EXISTS idx_request_status ON Request(status);

CREATE TABLE IF NOT EXISTS Plan (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  componentId TEXT NULL,
  repoId TEXT NULL,
  status TEXT NOT NULL,
  linkedRequestId TEXT NULL,
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
  FOREIGN KEY(linkedRequestId) REFERENCES Request(id)
);
CREATE INDEX IF NOT EXISTS idx_plan_status ON Plan(status);
CREATE INDEX IF NOT EXISTS idx_plan_componentId ON Plan(componentId);

CREATE TABLE IF NOT EXISTS PlanSection (
  id TEXT PRIMARY KEY,
  planId TEXT NOT NULL,
  label TEXT NOT NULL,
  content TEXT NOT NULL,
  sortOrder INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(planId) REFERENCES Plan(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_plansection_planId ON PlanSection(planId);

CREATE TABLE IF NOT EXISTS PlanPolicyCheck (
  id TEXT PRIMARY KEY,
  planId TEXT NOT NULL,
  rule TEXT NOT NULL,
  status TEXT NOT NULL,
  met INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(planId) REFERENCES Plan(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_planpolicycheck_planId ON PlanPolicyCheck(planId);

CREATE TABLE IF NOT EXISTS PlanApprover (
  id TEXT PRIMARY KEY,
  planId TEXT NOT NULL,
  role TEXT NOT NULL,
  name TEXT NOT NULL,
  approverStatus TEXT NOT NULL,
  FOREIGN KEY(planId) REFERENCES Plan(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_planapprover_planId ON PlanApprover(planId);

CREATE TABLE IF NOT EXISTS ExecutionRecord (
  id TEXT PRIMARY KEY,
  instanceId TEXT NULL,
  planId TEXT NULL,
  workflowId TEXT NULL,
  planTitle TEXT NULL,
  repoId TEXT NULL,
  componentId TEXT NULL,
  stage TEXT NULL,
  status TEXT NOT NULL,
  policyLevel TEXT NULL,
  startedBy TEXT NULL,
  waitingOn TEXT NULL,
  testResult TEXT NULL,
  prStatus TEXT NULL,
  prLink TEXT NULL,
  deployStatus TEXT NULL,
  createdAt TEXT NOT NULL,
  startedAt TEXT NULL,
  FOREIGN KEY(planId) REFERENCES Plan(id),
  FOREIGN KEY(repoId) REFERENCES Repo(id),
  FOREIGN KEY(componentId) REFERENCES Component(id)
);
CREATE INDEX IF NOT EXISTS idx_executionrecord_planId ON ExecutionRecord(planId);
CREATE INDEX IF NOT EXISTS idx_executionrecord_status ON ExecutionRecord(status);

CREATE TABLE IF NOT EXISTS Indicator (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  scope TEXT NOT NULL,
  weight REAL NOT NULL,
  threshold REAL NOT NULL,
  current REAL NOT NULL,
  trend REAL NOT NULL,
  reports INTEGER NOT NULL,
  mode TEXT NOT NULL,
  lastRun TEXT NOT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS Regression (
  id TEXT PRIMARY KEY,
  repoName TEXT NOT NULL,
  indicator TEXT NOT NULL,
  delta REAL NOT NULL,
  severity TEXT NOT NULL,
  since TEXT NOT NULL,
  trend TEXT NOT NULL,
  createdAt TEXT NOT NULL,
  FOREIGN KEY(repoName) REFERENCES Repo(name)
);
CREATE INDEX IF NOT EXISTS idx_regression_repoName ON Regression(repoName);

CREATE TABLE IF NOT EXISTS RecentAction (
  id TEXT PRIMARY KEY,
  time TEXT NOT NULL,
  action TEXT NOT NULL,
  subject TEXT NOT NULL,
  type TEXT NOT NULL,
  createdAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS SavedView (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  filters TEXT NULL,
  sort TEXT NULL,
  sortDir TEXT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_savedview_kind ON SavedView(kind);

CREATE TABLE IF NOT EXISTS Operator (
  id TEXT PRIMARY KEY,
  operatorType TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  paramsSchema TEXT NOT NULL,
  paletteLabel TEXT NULL,
  paletteIcon TEXT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS Persistence (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  createdAt TEXT NOT NULL,
  updatedAt TEXT NOT NULL
);

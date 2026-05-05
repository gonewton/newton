export type WorkflowStatus = 'pending' | 'running' | 'completed' | 'failed' | 'paused';
export type RiskLevel = 'low' | 'medium' | 'high' | 'critical';

export interface WorkflowSummary {
  workflow_id: string;
  instance_id: string;
  name: string;
  status: WorkflowStatus;
  created_at: string;
  updated_at: string;
}

export interface WorkflowDetail extends WorkflowSummary {
  description?: string;
  tasks: TaskSummary[];
}

export interface TaskSummary {
  task_id: string;
  name: string;
  status: WorkflowStatus;
  agent?: string;
  risk?: RiskLevel;
  score?: number;
}

export interface HILRequest {
  request_id: string;
  workflow_id: string;
  task_id: string;
  prompt: string;
  created_at: string;
}

export interface LogEntry {
  timestamp: string;
  level: 'debug' | 'info' | 'warn' | 'error';
  message: string;
  context?: Record<string, unknown>;
}

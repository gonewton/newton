export interface WorkflowDocument {
  version: string;
  workflow_id: string;
  name: string;
  description?: string;
  tasks: WorkflowTask[];
  settings?: WorkflowSettings;
}

export interface WorkflowTask {
  task_id: string;
  name: string;
  operator: string;
  depends_on?: string[];
  config?: Record<string, unknown>;
}

export interface WorkflowSettings {
  max_workflow_iterations?: number;
  max_task_iterations?: number;
  timeout_seconds?: number;
}

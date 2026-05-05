import type { WorkflowSummary } from '../types';

type Listener = () => void;

export class WorkflowStore {
  private workflows: WorkflowSummary[] = [];
  private listeners = new Set<Listener>();

  getAll(): WorkflowSummary[] {
    return this.workflows;
  }

  set(workflows: WorkflowSummary[]): void {
    this.workflows = workflows;
    this.emit();
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(): void {
    for (const l of this.listeners) l();
  }
}

import type { WorkflowSummary, WorkflowDetail, HILRequest } from '../types';

export interface ApiClientConfig {
  baseUrl: string;
  fetch?: typeof fetch;
}

export class ApiClient {
  private baseUrl: string;
  private fetchFn: typeof fetch;

  constructor(config: ApiClientConfig) {
    this.baseUrl = config.baseUrl.replace(/\/$/, '');
    this.fetchFn = config.fetch ?? globalThis.fetch.bind(globalThis);
  }

  async listWorkflows(): Promise<WorkflowSummary[]> {
    const res = await this.fetchFn(`${this.baseUrl}/api/workflows`);
    if (!res.ok) throw new Error(`listWorkflows failed: ${res.status}`);
    return res.json();
  }

  async getWorkflow(id: string): Promise<WorkflowDetail> {
    const res = await this.fetchFn(`${this.baseUrl}/api/workflows/${id}`);
    if (!res.ok) throw new Error(`getWorkflow failed: ${res.status}`);
    return res.json();
  }

  async listHilRequests(): Promise<HILRequest[]> {
    const res = await this.fetchFn(`${this.baseUrl}/api/hil/requests`);
    if (!res.ok) throw new Error(`listHilRequests failed: ${res.status}`);
    return res.json();
  }
}

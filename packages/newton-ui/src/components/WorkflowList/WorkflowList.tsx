import { StatusChip } from '@newton/design-system';
import { useWorkflows } from '../../hooks/useWorkflows';

export function WorkflowList() {
  const { data, loading, error } = useWorkflows();
  if (loading) return <div>Loading…</div>;
  if (error) return <div role="alert">Error: {error.message}</div>;
  return (
    <ul className="newton-workflow-list">
      {data.map((wf) => (
        <li key={wf.workflow_id} data-workflow-id={wf.workflow_id}>
          <span>{wf.name}</span>
          <StatusChip status={wf.status} />
        </li>
      ))}
    </ul>
  );
}

import { useEffect, useState } from 'react';
import { StatusChip, RiskChip, ScoreBar } from '@newton/design-system';
import { useNewton } from '../../context/NewtonProvider';
import type { WorkflowDetail as WorkflowDetailType } from '../../types';

export interface WorkflowDetailProps {
  workflowId: string;
}

export function WorkflowDetail({ workflowId }: WorkflowDetailProps) {
  const { api } = useNewton();
  const [data, setData] = useState<WorkflowDetailType | null>(null);

  useEffect(() => {
    api.getWorkflow(workflowId).then(setData).catch(() => setData(null));
  }, [api, workflowId]);

  if (!data) return <div>Loading…</div>;
  return (
    <section data-workflow-id={data.workflow_id}>
      <header>
        <h2>{data.name}</h2>
        <StatusChip status={data.status} />
      </header>
      <ul>
        {data.tasks.map((t) => (
          <li key={t.task_id}>
            <span>{t.name}</span>
            <StatusChip status={t.status} />
            {t.risk && <RiskChip risk={t.risk} />}
            {typeof t.score === 'number' && <ScoreBar value={t.score} />}
          </li>
        ))}
      </ul>
    </section>
  );
}

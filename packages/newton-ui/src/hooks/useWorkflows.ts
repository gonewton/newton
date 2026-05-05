import { useEffect, useState } from 'react';
import { useNewton } from '../context/NewtonProvider';
import type { WorkflowSummary } from '../types';

export function useWorkflows(): { data: WorkflowSummary[]; loading: boolean; error: Error | null } {
  const { api, store } = useNewton();
  const [data, setData] = useState<WorkflowSummary[]>(store.getAll());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .listWorkflows()
      .then((items) => {
        if (cancelled) return;
        store.set(items);
        setData(items);
        setLoading(false);
      })
      .catch((err: Error) => {
        if (cancelled) return;
        setError(err);
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [api, store]);

  return { data, loading, error };
}

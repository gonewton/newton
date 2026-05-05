import { useEffect, useState } from 'react';
import { useNewton } from '../../context/NewtonProvider';
import type { HILRequest } from '../../types';

export function HILPanel() {
  const { api } = useNewton();
  const [requests, setRequests] = useState<HILRequest[]>([]);

  useEffect(() => {
    api.listHilRequests().then(setRequests).catch(() => setRequests([]));
  }, [api]);

  return (
    <aside aria-label="Human in the loop">
      <h3>Pending HIL Requests</h3>
      <ul>
        {requests.map((r) => (
          <li key={r.request_id}>{r.prompt}</li>
        ))}
      </ul>
    </aside>
  );
}

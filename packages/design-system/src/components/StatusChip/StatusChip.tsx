import type { ReactNode } from 'react';

export interface StatusChipProps {
  status: string;
  children?: ReactNode;
}

export function StatusChip({ status, children }: StatusChipProps) {
  return (
    <span className="newton-status-chip" data-status={status}>
      {children ?? status}
    </span>
  );
}

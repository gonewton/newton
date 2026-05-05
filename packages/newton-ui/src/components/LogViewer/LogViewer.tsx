import type { LogEntry } from '../../types';

export interface LogViewerProps {
  entries: LogEntry[];
}

export function LogViewer({ entries }: LogViewerProps) {
  return (
    <pre className="newton-log-viewer">
      {entries.map((e, i) => (
        <div key={i} data-level={e.level}>
          [{e.timestamp}] {e.level.toUpperCase()} {e.message}
        </div>
      ))}
    </pre>
  );
}

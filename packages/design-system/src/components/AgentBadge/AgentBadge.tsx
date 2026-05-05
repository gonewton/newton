export interface AgentBadgeProps {
  name: string;
}

export function AgentBadge({ name }: AgentBadgeProps) {
  return <span className="newton-agent-badge">{name}</span>;
}

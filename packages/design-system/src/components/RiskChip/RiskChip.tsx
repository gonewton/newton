export interface RiskChipProps {
  level: 'low' | 'medium' | 'high';
}

export function RiskChip({ level }: RiskChipProps) {
  return (
    <span className="newton-risk-chip" data-level={level}>
      {level}
    </span>
  );
}

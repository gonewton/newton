export interface SkeletonBlockProps {
  width?: number | string;
  height?: number | string;
}

export function SkeletonBlock({ width = '100%', height = 16 }: SkeletonBlockProps) {
  return <div className="newton-skeleton" style={{ width, height }} />;
}

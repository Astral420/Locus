import React from "react";

export const ProgressBar: React.FC<{ value: number; label?: string; className?: string }> = ({
  value,
  label,
  className = "",
}) => {
  const pct = Math.min(100, Math.max(0, Number.isFinite(value) ? value : 0));
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      className={`h-1.5 w-full rounded-full bg-surface-hover overflow-hidden ${className}`}
    >
      <div className="h-full bg-primary transition-all duration-fast" style={{ width: `${pct}%` }} />
    </div>
  );
};

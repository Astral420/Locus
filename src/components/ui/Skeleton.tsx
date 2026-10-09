import React from "react";

export interface SkeletonProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
}

export const Skeleton: React.FC<SkeletonProps> = ({ className = "", ...props }) => {
  return (
    <div
      role="status"
      aria-busy="true"
      className={`animate-pulse bg-surface-hover rounded-lg ${className}`}
      {...props}
    >
      <span className="sr-only">Loading content…</span>
    </div>
  );
};

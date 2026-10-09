import React, { type ReactNode } from "react";
import { Button } from "./Button";

export interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description: string;
  actionLabel?: string;
  onAction?: () => void;
  className?: string;
  secondaryAction?: ReactNode;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  icon,
  title,
  description,
  actionLabel,
  onAction,
  className = "",
  secondaryAction,
}) => {
  return (
    <div
      className={`flex flex-col items-center justify-center text-center p-8 max-w-md mx-auto my-12 rounded-2xl bg-surface ${className}`}
      role="region"
      aria-label={title}
    >
      {icon && (
        <div className="w-12 h-12 rounded-full bg-surface-hover flex items-center justify-center text-ink-muted mb-4">
          {icon}
        </div>
      )}
      <h3 className="text-base font-semibold text-ink mb-1.5">{title}</h3>
      <p className="text-sm text-ink-muted leading-relaxed mb-6 max-w-xs">{description}</p>
      <div className="flex items-center gap-3">
        {actionLabel && onAction && (
          <Button variant="primary" onClick={onAction}>
            {actionLabel}
          </Button>
        )}
        {secondaryAction}
      </div>
    </div>
  );
};

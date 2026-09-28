import React from "react";

export interface StatusDotProps {
  status: "recording" | "online" | "warning" | "error" | "pending";
  size?: "sm" | "md" | "lg";
  className?: string;
  label?: string;
}

export const StatusDot: React.FC<StatusDotProps> = ({
  status,
  size = "md",
  className = "",
  label,
}) => {
  const sizeStyles = {
    sm: "w-2 h-2",
    md: "w-2.5 h-2.5",
    lg: "w-3 h-3",
  }[size];

  const dotStyles = {
    recording: "bg-status-recording animate-recording-pulse shadow-[0_0_8px_rgba(229,57,53,0.6)]",
    online: "bg-primary shadow-[0_0_6px_rgba(65,152,115,0.4)]",
    warning: "bg-status-warning",
    error: "bg-status-error",
    pending: "bg-status-pending",
  }[status];

  return (
    <span className={`inline-flex items-center gap-1.5 ${className}`}>
      <span className={`rounded-full shrink-0 ${sizeStyles} ${dotStyles}`} aria-hidden="true" />
      {label && <span className="text-xs text-ink-muted select-none">{label}</span>}
    </span>
  );
};

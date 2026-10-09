import React from "react";

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: "green" | "blue" | "amber" | "red" | "neutral";
  size?: "sm" | "md";
}

/** Soft rounded-rectangle badge (Jan's "Fits" / tag chips): tinted fill, no border. */
export const Badge: React.FC<BadgeProps> = ({
  variant = "neutral",
  size = "md",
  className = "",
  children,
  ...props
}) => {
  const baseStyles = "inline-flex items-center font-medium rounded-md nums-tabular";

  const variantStyles = {
    green: "bg-green-tint text-green-text",
    blue: "bg-blue-50 dark:bg-blue-950/60 text-blue-700 dark:text-blue-300",
    amber: "bg-amber-50 dark:bg-amber-950/60 text-amber-700 dark:text-amber-300",
    red: "bg-red-50 dark:bg-red-950/60 text-red-700 dark:text-red-300",
    neutral: "bg-surface-hover text-ink-muted",
  }[variant];

  const sizeStyles = {
    sm: "px-2 py-0.5 text-[11px] leading-tight",
    md: "px-2.5 py-1 text-xs leading-none",
  }[size];

  return (
    <span className={`${baseStyles} ${variantStyles} ${sizeStyles} ${className}`} {...props}>
      {children}
    </span>
  );
};

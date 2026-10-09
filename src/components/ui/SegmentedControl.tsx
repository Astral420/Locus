import React, { type ReactNode } from "react";

export interface SegmentedOption<T extends string> {
  value: T;
  label: ReactNode;
}

export interface SegmentedControlProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: SegmentedOption<T>[];
  "aria-label": string;
  className?: string;
}

/** Pill-shaped segmented control (tabs / short option sets). */
export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  className = "",
  ...rest
}: SegmentedControlProps<T>) {
  return (
    <div role="group" aria-label={rest["aria-label"]} className={`inline-flex gap-1 rounded-full bg-surface-hover p-1 ${className}`}>
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(o.value)}
            className={`h-7 px-3.5 rounded-full text-xs font-medium inline-flex items-center gap-1.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
              active ? "bg-border text-ink" : "text-ink-muted hover:text-ink"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

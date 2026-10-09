import React from "react";
import { ChevronsUpDown } from "lucide-react";

export interface SelectPillOption {
  value: string;
  label: string;
}

export interface SelectPillProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectPillOption[];
  "aria-label": string;
  className?: string;
}

/** Pill-shaped select (Jan's "Newest" / "Language" controls). Native <select> underneath for a11y. */
export const SelectPill: React.FC<SelectPillProps> = ({ value, onChange, options, className = "", ...rest }) => (
  <span className={`relative inline-flex items-center ${className}`}>
    <select
      aria-label={rest["aria-label"]}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="appearance-none h-8 pl-3.5 pr-8 rounded-full bg-surface-hover border border-border text-xs text-ink cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
    <ChevronsUpDown className="w-3.5 h-3.5 text-ink-muted absolute right-2.5 pointer-events-none" aria-hidden="true" />
  </span>
);

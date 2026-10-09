import React, { type ReactNode } from "react";

/** Grouped settings card: radius 12, one elevation step above the ground, no border. */
export const SettingsCard: React.FC<{ title?: string; children: ReactNode; className?: string }> = ({
  title,
  children,
  className = "",
}) => (
  <section className={`rounded-xl bg-surface px-5 py-4 ${className}`}>
    {title && <h2 className="text-base font-semibold text-ink mb-1">{title}</h2>}
    <div className="divide-y divide-border-subtle">{children}</div>
  </section>
);

export interface SettingsRowProps {
  label: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  children?: ReactNode;
}

/** Label + muted description on the left, control right-aligned, hairline divider between rows. */
export const SettingsRow: React.FC<SettingsRowProps> = ({ label, description, control, children }) => (
  <div className="py-3.5 first:pt-2">
    <div className="flex items-center justify-between gap-6">
      <div className="min-w-0">
        <div className="text-sm font-medium text-ink">{label}</div>
        {description && <div className="text-[13px] text-ink-muted mt-0.5 leading-snug">{description}</div>}
      </div>
      {control && <div className="shrink-0 flex items-center gap-2">{control}</div>}
    </div>
    {children && <div className="mt-3">{children}</div>}
  </div>
);

/** Uppercase 11px letter-spaced section label (Jan's "INTEGRATIONS", "MODEL PROVIDERS"). */
export const SectionLabel: React.FC<{ children: ReactNode; className?: string }> = ({ children, className = "" }) => (
  <div className={`px-3 pt-4 pb-1.5 text-[11px] font-medium uppercase tracking-[0.08em] text-ink-muted ${className}`}>
    {children}
  </div>
);

import React, { useEffect, useRef, type ReactNode } from "react";

export interface PopoverProps {
  open: boolean;
  onClose: () => void;
  "aria-label": string;
  children: ReactNode;
  className?: string;
}

/** Anchored card (Jan's download popover). Parent supplies a `relative` wrapper; closes on Escape or outside click. */
export const Popover: React.FC<PopoverProps> = ({ open, onClose, children, className = "", ...rest }) => {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onDown = (e: MouseEvent) => {
      const el = ref.current;
      if (el && !el.contains(e.target as Node) && !el.parentElement?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      ref={ref}
      role="dialog"
      aria-label={rest["aria-label"]}
      className={`absolute z-50 rounded-2xl border border-border bg-surface-elevated shadow-xl ${className}`}
    >
      {children}
    </div>
  );
};

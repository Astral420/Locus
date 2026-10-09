import React, { useState } from "react";
import { Check, Copy } from "lucide-react";

/** Monospace path chip with a copy button (Jan's data-folder path). */
export const PathChip: React.FC<{ path: string }> = ({ path }) => {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard
      ?.writeText(path)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => undefined);
  };
  return (
    <span className="inline-flex items-center gap-1.5 max-w-full">
      <code className="truncate rounded-md bg-surface-hover px-2 py-1 text-[11px] font-mono text-ink-muted">{path}</code>
      <button
        type="button"
        onClick={copy}
        aria-label="Copy path"
        className="shrink-0 rounded-md bg-surface-hover p-1.5 text-ink-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        {copied ? <Check className="w-3 h-3 text-primary" /> : <Copy className="w-3 h-3" />}
      </button>
    </span>
  );
};

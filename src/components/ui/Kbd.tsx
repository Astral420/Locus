import React from "react";

import { isMacPlatform } from "../../lib/platform";

export { isMacPlatform };

/** Dim right-aligned shortcut hint, e.g. "⌘ N" on macOS and "Ctrl N" elsewhere. */
export const Kbd: React.FC<{ keys: string; className?: string }> = ({ keys, className = "" }) => (
  <span aria-hidden="true" className={`text-[11px] tracking-wide text-ink-subtle nums-tabular ${className}`}>
    {isMacPlatform() ? "⌘" : "Ctrl"} {keys}
  </span>
);

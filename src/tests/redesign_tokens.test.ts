import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), "utf8");
const css = read("../styles.css");

const lightEnd = css.indexOf('[data-theme="dark"] {');
const lightBlock = css.slice(0, lightEnd);
const darkBlock = css.slice(lightEnd, css.indexOf("}", lightEnd));

const tokenValue = (block: string, name: string) => {
  const m = block.match(new RegExp(`--${name}:\\s*([^;]+);`));
  return m ? m[1].trim() : null;
};

// Brand + status + speaker tokens as they were before the redesign: the colour scheme must not drift.
const BRAND_LIGHT: Record<string, string> = {
  "color-primary": "oklch(0.612 0.118 158.0)",
  "color-primary-hover": "oklch(0.570 0.125 158.0)",
  "color-accent": "oklch(0.672 0.132 158.0)",
  "color-mint": "oklch(0.735 0.144 158.0)",
  "color-green-text": "oklch(0.420 0.095 158.0)",
  "color-green-tint": "oklch(0.945 0.035 158.0)",
  "color-status-recording": "oklch(0.62 0.22 25.0)",
  "color-status-error": "oklch(0.55 0.20 28.0)",
  "speaker-1-label": "#265c44",
  "speaker-2-pill": "#e3edf5",
};
const BRAND_DARK: Record<string, string> = {
  "color-primary": "oklch(0.735 0.144 158.0)",
  "color-primary-hover": "oklch(0.780 0.150 158.0)",
  "color-mint": "oklch(0.735 0.144 158.0)",
  "color-green-text": "oklch(0.780 0.130 158.0)",
  "color-green-tint": "oklch(0.250 0.050 158.0)",
  "color-status-recording": "oklch(0.68 0.22 25.0)",
  "color-status-error": "oklch(0.65 0.20 28.0)",
  "speaker-1-label": "#64d6a2",
  "speaker-2-pill": "#1a2f42",
};

describe("Redesign tokens: colour scheme retained", () => {
  it("keeps every brand, status and speaker token unchanged in light mode", () => {
    for (const [name, value] of Object.entries(BRAND_LIGHT)) {
      expect(tokenValue(lightBlock, name), name).toBe(value);
    }
  });

  it("keeps every brand, status and speaker token unchanged in dark mode", () => {
    for (const [name, value] of Object.entries(BRAND_DARK)) {
      expect(tokenValue(darkBlock, name), name).toBe(value);
    }
  });

  it("leaves the light paper neutrals untouched", () => {
    expect(tokenValue(lightBlock, "color-bg")).toBe("oklch(0.978 0.006 85.0)");
    expect(tokenValue(lightBlock, "color-surface")).toBe("oklch(0.952 0.008 85.0)");
    expect(tokenValue(lightBlock, "color-ink")).toBe("oklch(0.185 0.012 160.0)");
  });
});

describe("Redesign tokens: neutral graphite dark ground", () => {
  const neutrals = [
    "color-bg",
    "color-surface",
    "color-surface-elevated",
    "color-surface-sunken",
    "color-surface-hover",
    "color-border",
    "color-border-subtle",
    "color-ink",
    "color-ink-muted",
    "color-ink-subtle",
  ];

  it("uses zero-chroma neutrals (no green tint) for every dark surface, border and ink", () => {
    for (const name of neutrals) {
      const v = tokenValue(darkBlock, name);
      expect(v, name).not.toBeNull();
      expect(v, name).toMatch(/^oklch\(\d\.\d{3} 0\.000 [\d.]+\)$/);
    }
  });

  it("keeps the prefers-color-scheme fallback in sync with the explicit dark theme", () => {
    const mediaStart = css.indexOf("@media (prefers-color-scheme: dark)");
    const media = css.slice(mediaStart);
    for (const name of neutrals) {
      expect(tokenValue(media, name), name).toBe(tokenValue(darkBlock, name));
    }
  });

  it("defines hover and on-primary tokens for both themes", () => {
    expect(tokenValue(lightBlock, "color-surface-hover")).not.toBeNull();
    expect(tokenValue(lightBlock, "color-on-primary")).toBe("oklch(1 0 0)");
    expect(tokenValue(darkBlock, "color-on-primary")).toBe("oklch(0.203 0.023 165.2)");
  });
});

describe("Redesign tokens: contrast of the graphite palette", () => {
  const lum = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => {
      const v = parseInt(hex.slice(i, i + 2), 16) / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const contrast = (a: string, b: string) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };

  it("keeps body and secondary text accessible on the ground and on cards", () => {
    expect(contrast("#ededed", "#121212")).toBeGreaterThan(14);
    expect(contrast("#9b9b9b", "#121212")).toBeGreaterThan(6);
    expect(contrast("#9b9b9b", "#1a1a1a")).toBeGreaterThan(5.5);
    expect(contrast("#6b6b6b", "#121212")).toBeGreaterThan(3); // placeholders / decorative icons
  });

  it("uses dark text on the mint primary in dark mode (white would fail AA)", () => {
    expect(contrast("#0c1a14", "#52bf90")).toBeGreaterThan(7);
    expect(contrast("#ffffff", "#52bf90")).toBeLessThan(3);
  });

  it("keeps green text readable on its dark tint", () => {
    expect(contrast("#64d6a2", "#173629")).toBeGreaterThan(4.5);
  });
});

describe("Redesign tokens: new components only use tokens", () => {
  const files = [
    "../components/ui/Toggle.tsx",
    "../components/ui/SelectPill.tsx",
    "../components/ui/SettingsCard.tsx",
    "../components/ui/Kbd.tsx",
    "../components/ui/PathChip.tsx",
    "../components/ui/SegmentedControl.tsx",
    "../components/ui/ProgressBar.tsx",
    "../components/ui/Popover.tsx",
    "../components/layout/Sidebar.tsx",
    "../components/layout/Header.tsx",
  ];

  it("contains no raw hex or rgb colours in the new/rewritten components", () => {
    for (const file of files) {
      const src = read(file);
      expect(src, file).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
      expect(src, file).not.toMatch(/\brgba?\(/);
    }
  });
});

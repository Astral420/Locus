import React from "react";
import { Moon, Sun, Monitor, Circle, Search, PlusCircle } from "lucide-react";
import { useUiStore, type ThemeMode } from "../../stores/uiStore";
import { useRecordingStore } from "../../stores/recordingStore";
import { useNavigate } from "@tanstack/react-router";
import { formatElapsed } from "../../lib/format";

export interface HeaderProps {
  title?: string;
  subtitle?: string;
  actions?: React.ReactNode;
}

export const Header: React.FC<HeaderProps> = ({ title, subtitle, actions }) => {
  const { theme, setTheme } = useUiStore();
  const { state: recordingState, elapsed_seconds } = useRecordingStore();
  const navigate = useNavigate();

  const nextTheme: Record<ThemeMode, ThemeMode> = {
    system: "light",
    light: "dark",
    dark: "system",
  };

  const ThemeIcon = {
    system: Monitor,
    light: Sun,
    dark: Moon,
  }[theme];

  return (
    <header className="h-14 border-b border-border/80 bg-surface/40 px-6 flex items-center justify-between gap-4 sticky top-0 z-20 backdrop-blur-sm">
      {/* Title & Context */}
      <div className="flex items-center gap-3 min-w-0">
        <div>
          {title && (
            <h1 className="text-sm font-semibold text-ink tracking-tight truncate">{title}</h1>
          )}
          {subtitle && (
            <p className="text-[11px] text-ink-muted tracking-tight truncate">{subtitle}</p>
          )}
        </div>
      </div>

      {/* Center Active Recording HUD Banner if recording is active outside of /record */}
      {(recordingState === "recording" || recordingState === "paused") && (
        <div
          onClick={() => void navigate({ to: "/record" })}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") void navigate({ to: "/record" });
          }}
          className="flex items-center gap-2 px-3 py-1 bg-red-500/10 border border-status-recording/30 rounded-full cursor-pointer hover:bg-red-500/20 text-xs font-mono font-medium text-status-recording focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-recording"
        >
          <Circle
            className={`w-2.5 h-2.5 fill-status-recording text-status-recording ${recordingState === "recording" ? "animate-ping" : ""}`}
          />
          <span>{recordingState === "recording" ? "REC" : "PAUSED"}</span>
          <span className="tabular-nums font-semibold">{formatElapsed(elapsed_seconds)}</span>
        </div>
      )}

      {/* Header Actions */}
      <div className="flex items-center gap-2 shrink-0">
        {actions}

        {/* Global Quick Action: New Recording */}
        <button
          type="button"
          onClick={() => void navigate({ to: "/record" })}
          title="Start Recording (Cmd/Ctrl + N)"
          aria-label="Start a new recording"
          className="h-8 px-2.5 rounded flex items-center gap-1.5 text-xs font-medium text-ink-muted hover:text-ink hover:bg-surface-sunken focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          <PlusCircle className="w-4 h-4 text-primary" />
          <span className="hidden sm:inline">New Record</span>
        </button>

        {/* Theme Switcher */}
        <button
          type="button"
          onClick={() => setTheme(nextTheme[theme])}
          title={`Theme: ${theme}. Click to switch.`}
          aria-label={`Current theme is ${theme}. Click to switch theme.`}
          className="w-8 h-8 rounded flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-sunken focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          <ThemeIcon className="w-4 h-4" />
        </button>
      </div>
    </header>
  );
};

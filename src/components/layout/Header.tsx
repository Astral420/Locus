import React from "react";
import { Circle } from "lucide-react";
import { useRecordingStore } from "../../stores/recordingStore";
import { useNavigate } from "@tanstack/react-router";
import { formatElapsed } from "../../lib/format";
import { hiddenSidebarInset } from "../../lib/platform";
import { useUiStore } from "../../stores/uiStore";

export interface HeaderProps {
  title?: string;
  subtitle?: string;
  actions?: React.ReactNode;
}

/** Borderless page header (Jan style): plain title, soft REC pill, page-specific actions. Theme lives in Settings → Appearance. */
export const Header: React.FC<HeaderProps> = ({ title, subtitle, actions }) => {
  const { state: recordingState, elapsed_seconds } = useRecordingStore();
  const navigate = useNavigate();
  const sidebarHidden = useUiStore((s) => s.sidebarCollapsed);

  return (
    <header
      data-tauri-drag-region
      className={`h-14 px-6 ${hiddenSidebarInset(sidebarHidden)} flex items-center justify-between gap-4 sticky top-0 z-20 bg-bg/90 backdrop-blur-sm`}
    >
      {/* Title & context */}
      <div data-tauri-drag-region className="flex items-baseline gap-3 min-w-0">
        {title && <h1 className="text-[15px] font-semibold text-ink tracking-tight truncate">{title}</h1>}
        {subtitle && <p className="hidden lg:block text-[13px] text-ink-muted truncate">{subtitle}</p>}
      </div>

      {/* Active recording pill (shown on every screen while capture is running) */}
      {(recordingState === "recording" || recordingState === "paused") && (
        <div
          onClick={() => void navigate({ to: "/record" })}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") void navigate({ to: "/record" });
          }}
          className="flex items-center gap-2 px-3 py-1 bg-status-recording/10 rounded-full cursor-pointer hover:bg-status-recording/20 text-xs font-mono font-medium text-status-recording focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-recording"
        >
          <Circle
            className={`w-2.5 h-2.5 fill-status-recording text-status-recording ${recordingState === "recording" ? "animate-ping" : ""}`}
          />
          <span>{recordingState === "recording" ? "REC" : "PAUSED"}</span>
          <span className="tabular-nums font-semibold">{formatElapsed(elapsed_seconds)}</span>
        </div>
      )}

      {/* Header actions */}
      <div className="flex items-center gap-2 shrink-0">
        {actions}
      </div>
    </header>
  );
};

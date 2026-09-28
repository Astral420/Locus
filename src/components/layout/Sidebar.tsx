import React from "react";
import { Link, useRouterState, useNavigate } from "@tanstack/react-router";
import {
  Disc,
  FolderKanban,
  BookOpen,
  Cpu,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  ArrowDownToLine,
} from "lucide-react";
import { useUiStore } from "../../stores/uiStore";
import { useRecordingStore } from "../../stores/recordingStore";

export const Sidebar: React.FC = () => {
  const routerState = useRouterState();
  const navigate = useNavigate();
  const currentPath = routerState.location.pathname;

  const { sidebarCollapsed, toggleSidebar, downloadStatus } = useUiStore();
  const recordingState = useRecordingStore((s) => s.state);

  const navItems = [
    {
      to: "/record",
      label: "Recording",
      icon: Disc,
      badge: recordingState === "recording" ? "LIVE" : undefined,
    },
    {
      to: "/",
      label: "Meetings",
      icon: FolderKanban,
    },
    {
      to: "/knowledge",
      label: "Knowledge Base",
      icon: BookOpen,
    },
    {
      to: "/models",
      label: "Model Manager",
      icon: Cpu,
    },
    {
      to: "/settings",
      label: "Settings",
      icon: Settings,
    },
  ];

  return (
    <aside
      aria-label="Primary navigation"
      className={`h-screen sticky top-0 flex flex-col bg-surface/50 border-r border-border transition-all duration-standard select-none z-30 shrink-0 ${
        sidebarCollapsed ? "w-sidebar-collapsed" : "w-sidebar"
      }`}
    >
      {/* Brand Header */}
      <div className="h-14 flex items-center justify-between px-3.5 border-b border-border/60">
        {!sidebarCollapsed && (
          <div className="flex items-center gap-2 overflow-hidden">
            <span className="w-5 h-5 rounded bg-primary flex items-center justify-center text-white font-mono text-[10px] font-bold">
              L
            </span>
            <span className="font-semibold text-sm tracking-wider text-ink">LOCUS</span>
          </div>
        )}
        <button
          type="button"
          onClick={toggleSidebar}
          aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          className={`p-1.5 rounded text-ink-muted hover:text-ink hover:bg-surface-sunken focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
            sidebarCollapsed ? "mx-auto" : ""
          }`}
        >
          {sidebarCollapsed ? (
            <PanelLeftOpen className="w-4 h-4" />
          ) : (
            <PanelLeftClose className="w-4 h-4" />
          )}
        </button>
      </div>

      {/* Nav List */}
      <nav className="flex-1 py-3 px-2 space-y-1 overflow-y-auto">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive =
            item.to === "/"
              ? currentPath === "/" || currentPath.startsWith("/meeting")
              : currentPath === item.to || currentPath.startsWith(`${item.to}/`);

          return (
            <Link
              key={item.to}
              to={item.to}
              title={sidebarCollapsed ? item.label : undefined}
              aria-current={isActive ? "page" : undefined}
              className={`relative flex items-center h-8 px-2.5 rounded text-xs font-medium transition-colors ${
                isActive
                  ? "bg-green-tint text-green-text font-semibold"
                  : "text-ink-muted hover:text-ink hover:bg-surface"
              } ${sidebarCollapsed ? "justify-center px-0" : "gap-2.5"}`}
            >
              {/* Active left 3px indicator pill */}
              {isActive && (
                <span
                  className="absolute left-0 top-1 bottom-1 w-[3px] bg-primary rounded-r"
                  aria-hidden="true"
                />
              )}
              <Icon
                className={`w-4 h-4 shrink-0 ${
                  isActive
                    ? "text-primary"
                    : item.to === "/record" && recordingState === "recording"
                    ? "text-status-recording animate-pulse"
                    : "text-ink-muted"
                }`}
              />
              {!sidebarCollapsed && (
                <span className="truncate flex-1 text-left">{item.label}</span>
              )}
              {!sidebarCollapsed && item.badge && (
                <span className="px-1.5 py-0.2 text-[10px] font-bold font-mono tracking-wider rounded bg-status-recording text-white animate-pulse">
                  {item.badge}
                </span>
              )}
            </Link>
          );
        })}
      </nav>

      {/* Sidebar Footer */}
      <div className="p-3 border-t border-border/60 bg-surface/70 space-y-2">
        {/* Background download progress indicator */}
        {downloadStatus.active && (
          <div
            onClick={() => void navigate({ to: "/models" })}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") void navigate({ to: "/models" });
            }}
            title="Download in progress. Click to open Model Manager."
            aria-label={`Downloading ${downloadStatus.modelName}: ${downloadStatus.progress} percent`}
            className="p-2 rounded bg-surface-sunken hover:bg-surface border border-border/80 cursor-pointer text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <div className="flex items-center justify-between text-[11px] font-mono text-ink-muted mb-1">
              <span className="flex items-center gap-1.5 truncate">
                <ArrowDownToLine className="w-3 h-3 text-primary animate-bounce shrink-0" />
                {!sidebarCollapsed && <span className="truncate">{downloadStatus.modelName}</span>}
              </span>
              <span className="tabular-nums shrink-0 font-medium text-ink">
                {downloadStatus.progress}%
              </span>
            </div>
            <div className="w-full h-1.5 bg-border rounded-full overflow-hidden">
              <div
                className="h-full bg-primary transition-all duration-fast"
                style={{ width: `${downloadStatus.progress}%` }}
              />
            </div>
          </div>
        )}

        {/* Offline Engine status pill */}
        <div
          className={`flex items-center gap-2 py-1 px-1.5 rounded text-[11px] font-mono text-green-text ${
            sidebarCollapsed ? "justify-center" : ""
          }`}
          title="Engine is running offline on your local machine with zero network telemetry."
        >
          <span className="relative flex h-2 w-2 shrink-0">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-primary opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-primary" />
          </span>
          {!sidebarCollapsed && (
            <span className="truncate tracking-tight font-medium">Offline · Local Engine</span>
          )}
        </div>
      </div>
    </aside>
  );
};

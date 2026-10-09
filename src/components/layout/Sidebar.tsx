import React, { useState } from "react";
import { Link, useNavigate, useRouterState } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Disc,
  FolderKanban,
  BookOpen,
  Cpu,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  Download,
  Plus,
} from "lucide-react";
import { useUiStore } from "../../stores/uiStore";
import { useRecordingStore } from "../../stores/recordingStore";
import { useKnowledgeStore } from "../../stores/knowledgeStore";
import { listKnowledgeThreads, listModels, type KnowledgeThreadDTO, type ModelAssetDTO } from "../../lib/tauri";
import { KNOWLEDGE_SCOPE_LABELS, openKnowledgeThread } from "../../lib/knowledgeThreads";
import { hasOverlayTitleBar } from "../../lib/platform";
import { Kbd } from "../ui/Kbd";
import { Popover } from "../ui/Popover";
import { ProgressBar } from "../ui/ProgressBar";

interface ActiveDownload {
  id: string;
  name: string;
  progress: number;
}

export const DownloadsButton: React.FC = () => {
  const [open, setOpen] = useState(false);
  const downloadStatus = useUiStore((s) => s.downloadStatus);
  const { data: models = [] } = useQuery<ModelAssetDTO[]>({ queryKey: ["models"], queryFn: listModels });

  const downloads: ActiveDownload[] = [
    ...models
      .filter((m) => m.status === "downloading")
      .map((m) => ({ id: m.id, name: m.name, progress: m.download_progress ?? 0 })),
    ...(downloadStatus.active
      ? [{ id: "ui-store", name: downloadStatus.modelName, progress: downloadStatus.progress }]
      : []),
  ];

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-label="Downloads"
        aria-haspopup="dialog"
        aria-expanded={open}
        title="Downloads"
        className={`relative w-8 h-8 rounded-lg flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
          open ? "bg-surface-hover text-ink" : ""
        }`}
      >
        <Download className="w-4 h-4" />
        {downloads.length > 0 && (
          <span aria-hidden="true" className="absolute top-1.5 right-1.5 w-1.5 h-1.5 rounded-full bg-primary" />
        )}
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        aria-label="Download progress"
        className="top-full left-0 mt-2 w-72 p-4"
      >
        {downloads.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-5 text-center text-[13px] text-ink-muted">
            <Download className="w-5 h-5 text-ink-subtle" aria-hidden="true" />
            <p>
              Your download progress
              <br />
              will appear here
            </p>
          </div>
        ) : (
          <ul className="space-y-3">
            {downloads.map((d) => (
              <li key={d.id} className="space-y-1.5">
                <div className="flex items-center justify-between gap-3 text-xs">
                  <span className="truncate text-ink font-medium">{d.name}</span>
                  <span className="nums-tabular text-ink-muted shrink-0">{Math.round(d.progress)}%</span>
                </div>
                <ProgressBar value={d.progress} label={`Downloading ${d.name}`} />
              </li>
            ))}
          </ul>
        )}
      </Popover>
    </div>
  );
};

/** Floating controls shown while the sidebar is hidden: reopen + downloads, beside the macOS traffic lights. */
const HiddenSidebarControls: React.FC = () => {
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const overlay = hasOverlayTitleBar();
  return (
    <div
      className={`fixed top-3 z-40 flex items-center gap-0.5 ${overlay ? "left-[88px]" : "left-3"}`}
      role="toolbar"
      aria-label="Sidebar controls"
    >
      <button
        type="button"
        onClick={toggleSidebar}
        aria-label="Show sidebar"
        title="Show sidebar"
        className="w-8 h-8 rounded-lg flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <PanelLeftOpen className="w-4 h-4" />
      </button>
      <DownloadsButton />
    </div>
  );
};

const ConversationList: React.FC<{ currentPath: string }> = ({ currentPath }) => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const selectedThreadId = useKnowledgeStore((s) => s.selectedThreadId);
  const setSelectedThreadId = useKnowledgeStore((s) => s.setSelectedThreadId);
  const { data: threads = [] } = useQuery<KnowledgeThreadDTO[]>({
    queryKey: ["knowledgeThreads"],
    queryFn: listKnowledgeThreads,
  });

  const onKnowledge = currentPath === "/knowledge" || currentPath.startsWith("/knowledge/");
  const effectiveId = selectedThreadId ?? threads[0]?.id ?? null;

  const startNew = () => {
    void openKnowledgeThread(queryClient, threads, "all_meetings", null)
      .then(() => navigate({ to: "/knowledge" }))
      .catch(() => undefined);
  };

  return (
    <section aria-label="Conversations" className="flex-1 min-h-0 flex flex-col px-2 pt-2">
      <div className="flex items-center justify-between px-2.5 pb-1">
        <span className="text-[11px] font-medium uppercase tracking-[0.08em] text-ink-muted">Conversations</span>
        <button
          type="button"
          onClick={startNew}
          aria-label="New conversation"
          title="New conversation"
          className="w-6 h-6 rounded-full flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          <Plus className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* The list scrolls inside the sidebar; the navigation above it stays fixed. */}
      <ul className="flex-1 min-h-0 overflow-y-auto space-y-0.5 pb-2" aria-label="Conversation history">
        {threads.length === 0 && <li className="px-2.5 py-1.5 text-xs text-ink-subtle">No conversations yet</li>}
        {threads.map((thread) => {
          const isActive = onKnowledge && thread.id === effectiveId;
          return (
            <li key={thread.id}>
              <button
                type="button"
                aria-current={isActive ? "true" : undefined}
                onClick={() => {
                  setSelectedThreadId(thread.id);
                  void navigate({ to: "/knowledge" });
                }}
                className={`w-full text-left px-2.5 py-1.5 rounded-[10px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                  isActive ? "bg-surface-hover text-ink" : "text-ink-muted hover:text-ink hover:bg-surface-hover/60"
                }`}
              >
                <span className="block truncate text-[13px]">{thread.title}</span>
                <span className="block truncate text-[11px] text-ink-subtle">
                  {KNOWLEDGE_SCOPE_LABELS[thread.scope]} · {thread.message_count} {thread.message_count === 1 ? "message" : "messages"}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
};

export const Sidebar: React.FC = () => {
  const routerState = useRouterState();
  const currentPath = routerState.location.pathname;

  const { sidebarCollapsed, toggleSidebar } = useUiStore();
  const recordingState = useRecordingStore((s) => s.state);
  const overlay = hasOverlayTitleBar();

  // Hidden entirely (no icon rail); a small control beside the traffic lights brings it back.
  if (sidebarCollapsed) return <HiddenSidebarControls />;

  const navItems = [
    { to: "/record", label: "Recording", icon: Disc, hint: "N", badge: recordingState === "recording" ? "LIVE" : undefined },
    { to: "/", label: "Meetings", icon: FolderKanban, hint: "K" },
    { to: "/knowledge", label: "Knowledge Base", icon: BookOpen },
    { to: "/models", label: "Model Manager", icon: Cpu },
    { to: "/settings", label: "Settings", icon: Settings, hint: "," },
  ];

  return (
    <aside
      aria-label="Primary navigation"
      className="sticky top-2 m-2 h-[calc(100vh-1rem)] w-[232px] flex flex-col rounded-[14px] border border-border bg-bg select-none z-30 shrink-0"
    >
      {/* Top row (centre line shared with the page title and the macOS traffic lights): downloads + hide sidebar */}
      <div data-tauri-drag-region className="flex items-center justify-between px-3 h-[38px] shrink-0">
        {overlay ? <div data-tauri-drag-region aria-hidden="true" className="w-[68px] h-7" /> : null}
        <div className="flex items-center gap-0.5 ml-auto">
          <DownloadsButton />
          <button
            type="button"
            onClick={toggleSidebar}
            aria-label="Hide sidebar"
            title="Hide sidebar"
            className="w-8 h-8 rounded-lg flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <PanelLeftClose className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Nav list */}
      <nav className="px-2 pt-2 space-y-0.5 shrink-0">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive =
            item.to === "/"
              ? currentPath === "/" || currentPath.startsWith("/meeting")
              : currentPath === item.to || currentPath.startsWith(`${item.to}/`);
          const isLiveRecord = item.to === "/record" && recordingState === "recording";

          return (
            <Link
              key={item.to}
              to={item.to}
              aria-current={isActive ? "page" : undefined}
              className={`flex items-center h-9 gap-2.5 px-2.5 rounded-[10px] text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                isActive ? "bg-surface-hover text-ink" : "text-ink-muted hover:text-ink hover:bg-surface-hover/60"
              }`}
            >
              <Icon
                className={`w-4 h-4 shrink-0 ${
                  isLiveRecord ? "text-status-recording animate-pulse" : isActive ? "text-ink" : "text-ink-muted"
                }`}
              />
              <span className="truncate flex-1 text-left">{item.label}</span>
              {item.badge && (
                <span className="px-1.5 py-px text-[10px] font-bold font-mono tracking-wider rounded bg-status-recording text-white animate-pulse">
                  {item.badge}
                </span>
              )}
              {!item.badge && item.hint && <Kbd keys={item.hint} />}
            </Link>
          );
        })}
      </nav>

      <ConversationList currentPath={currentPath} />
    </aside>
  );
};

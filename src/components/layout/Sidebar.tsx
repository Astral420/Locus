import React, { useEffect, useRef, useState } from "react";
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
  MoreVertical,
  Trash2,
} from "lucide-react";
import { useUiStore } from "../../stores/uiStore";
import { useRecordingStore } from "../../stores/recordingStore";
import { useKnowledgeStore } from "../../stores/knowledgeStore";
import { deleteKnowledgeThread, listKnowledgeThreads, listModels, type KnowledgeThreadDTO, type ModelAssetDTO } from "../../lib/tauri";
import { KNOWLEDGE_SCOPE_LABELS, openKnowledgeThread } from "../../lib/knowledgeThreads";
import { hasOverlayTitleBar } from "../../lib/platform";
import { Kbd } from "../ui/Kbd";
import { Popover } from "../ui/Popover";
import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";
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
      <DownloadsButton />
      <button
        type="button"
        onClick={toggleSidebar}
        aria-label="Show sidebar"
        title="Show sidebar"
        className="w-8 h-8 rounded-lg flex items-center justify-center text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <PanelLeftOpen className="w-4 h-4" />
      </button>
    </div>
  );
};

interface ConversationRowProps {
  thread: KnowledgeThreadDTO;
  isActive: boolean;
  onOpen: () => void;
  onRequestDelete: (thread: KnowledgeThreadDTO) => void;
}

/** One conversation: click to open, kebab (⋮) menu to delete. The menu is fixed-positioned so the scrolling list never clips it. */
const ConversationRow: React.FC<ConversationRowProps> = ({ thread, isActive, onOpen, onRequestDelete }) => {
  const [menu, setMenu] = useState<{ top: number; left: number } | null>(null);
  const kebabRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMenu(null);
        kebabRef.current?.focus();
      }
    };
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!menuRef.current?.contains(target) && !kebabRef.current?.contains(target)) setMenu(null);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("resize", close);
    document.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("resize", close);
      document.removeEventListener("scroll", close, true);
    };
  }, [menu]);

  const toggleMenu = () => {
    if (menu) {
      setMenu(null);
      return;
    }
    const rect = kebabRef.current?.getBoundingClientRect();
    setMenu({ top: (rect?.bottom ?? 0) + 4, left: Math.max(8, (rect?.right ?? 180) - 176) });
  };

  return (
    <li className="group relative">
      <button
        type="button"
        aria-current={isActive ? "true" : undefined}
        onClick={onOpen}
        className={`w-full text-left pl-2.5 pr-9 py-1.5 rounded-[10px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
          isActive ? "bg-surface-hover text-ink" : "text-ink-muted hover:text-ink hover:bg-surface-hover/60"
        }`}
      >
        <span className="block truncate text-[13px]">{thread.title}</span>
        <span className="block truncate text-[11px] text-ink-subtle">
          {KNOWLEDGE_SCOPE_LABELS[thread.scope]} · {thread.message_count} {thread.message_count === 1 ? "message" : "messages"}
        </span>
      </button>

      <button
        ref={kebabRef}
        type="button"
        aria-label="Conversation options"
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        onClick={toggleMenu}
        className="absolute right-1.5 top-1/2 -translate-y-1/2 w-6 h-6 rounded-full flex items-center justify-center text-ink-muted hover:text-ink hover:bg-border opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 aria-expanded:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <MoreVertical className="w-3.5 h-3.5" />
      </button>

      {menu && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="Conversation options"
          style={{ top: menu.top, left: menu.left }}
          className="fixed z-50 w-44 rounded-xl border border-border bg-surface-elevated p-1 shadow-xl"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              onRequestDelete(thread);
            }}
            className="w-full h-8 px-2.5 rounded-lg flex items-center gap-2 text-[13px] text-status-error hover:bg-status-error/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Delete conversation
          </button>
        </div>
      )}
    </li>
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

  const [pendingDelete, setPendingDelete] = useState<KnowledgeThreadDTO | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const onKnowledge = currentPath === "/knowledge" || currentPath.startsWith("/knowledge/");
  const effectiveId = selectedThreadId ?? threads[0]?.id ?? null;

  const startNew = () => {
    void openKnowledgeThread(queryClient, threads, "all_meetings", null)
      .then(() => navigate({ to: "/knowledge" }))
      .catch(() => undefined);
  };

  const closeDeleteDialog = () => {
    if (isDeleting) return;
    setPendingDelete(null);
    setDeleteError(null);
  };

  const confirmDelete = () => {
    if (!pendingDelete) return;
    const target = pendingDelete;
    setIsDeleting(true);
    setDeleteError(null);
    deleteKnowledgeThread(target.id)
      .then(async () => {
        if (useKnowledgeStore.getState().selectedThreadId === target.id) setSelectedThreadId(null);
        queryClient.removeQueries({ queryKey: ["knowledgeMessages", target.id] });
        await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
        setPendingDelete(null);
      })
      .catch((error: unknown) => {
        setDeleteError(error instanceof Error ? error.message : typeof error === "string" ? error : "The conversation could not be deleted.");
      })
      .finally(() => setIsDeleting(false));
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
        {threads.map((thread) => (
          <ConversationRow
            key={thread.id}
            thread={thread}
            isActive={onKnowledge && thread.id === effectiveId}
            onOpen={() => {
              setSelectedThreadId(thread.id);
              void navigate({ to: "/knowledge" });
            }}
            onRequestDelete={setPendingDelete}
          />
        ))}
      </ul>

      <Modal
        isOpen={pendingDelete !== null}
        onClose={closeDeleteDialog}
        title="Delete conversation?"
        description={
          pendingDelete
            ? `“${pendingDelete.title}” and its ${pendingDelete.message_count} ${pendingDelete.message_count === 1 ? "message" : "messages"} will be permanently deleted. Your meetings and documents are not affected.`
            : undefined
        }
      >
        {deleteError && (
          <p role="alert" className="mb-3 rounded-xl bg-status-error/10 px-3 py-2 text-xs text-status-error">
            {deleteError}
          </p>
        )}
        <div className="flex items-center justify-end gap-2">
          <Button variant="secondary" size="sm" onClick={closeDeleteDialog} disabled={isDeleting}>
            Cancel
          </Button>
          <Button variant="danger" size="sm" onClick={confirmDelete} isLoading={isDeleting}>
            <Trash2 className="w-3.5 h-3.5" />
            Delete
          </Button>
        </div>
      </Modal>
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

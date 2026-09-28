import React, { useState, useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Skeleton } from "../components/ui/Skeleton";
import { EmptyState } from "../components/ui/EmptyState";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { DeleteMeetingModal } from "../components/meetings/DeleteMeetingModal";
import { listMeetings, type MeetingDTO } from "../lib/tauri";
import {
  FolderKanban,
  Search,
  Plus,
  Clock,
  Users,
  Video,
  ChevronRight,
  Filter,
  Calendar,
  Trash2,
  CheckCircle2,
} from "lucide-react";

const formatDuration = (seconds: number) => {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
};

const formatDate = (isoString: string) => {
  const date = new Date(isoString);
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
};

const MeetingsContent: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const [searchTerm, setSearchTerm] = useState("");
  const [typeFilter, setTypeFilter] = useState<string>("all");
  const [dateFilter, setDateFilter] = useState<string>("all");
  const [meetingToDelete, setMeetingToDelete] = useState<MeetingDTO | null>(null);

  const {
    data: meetings,
    isLoading,
    isError,
    refetch,
  } = useQuery<MeetingDTO[]>({
    queryKey: ["meetings"],
    queryFn: listMeetings,
  });

  // Global shortcut Cmd+K or Ctrl+K to focus search
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  if (isLoading) {
    return (
      <div className="p-6 space-y-4 max-w-5xl mx-auto w-full">
        <div className="flex items-center justify-between gap-4 mb-6">
          <Skeleton className="h-9 w-64" />
          <Skeleton className="h-9 w-32" />
        </div>
        {[1, 2, 3].map((i) => (
          <div key={i} className="p-4 border border-border rounded-lg bg-surface/40 space-y-3">
            <div className="flex items-center justify-between">
              <Skeleton className="h-5 w-72" />
              <Skeleton className="h-5 w-20" />
            </div>
            <div className="flex items-center gap-4">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-4 w-16" />
              <Skeleton className="h-4 w-20" />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (isError) {
    throw new Error("Unable to retrieve meeting catalog from local SQLite storage.");
  }

  const now = new Date().getTime();
  const ONE_DAY = 24 * 60 * 60 * 1000;

  const filteredMeetings = (meetings || []).filter((m) => {
    // Search match
    const query = searchTerm.toLowerCase();
    const matchesSearch =
      m.title.toLowerCase().includes(query) ||
      m.requested_type.toLowerCase().includes(query) ||
      (m.detected_type && m.detected_type.toLowerCase().includes(query));

    // Type filter
    const matchesType = typeFilter === "all" || m.requested_type === typeFilter;

    // Date range filter
    let matchesDate = true;
    if (dateFilter !== "all") {
      const recordedTime = new Date(m.recorded_at).getTime();
      const diff = now - recordedTime;
      if (dateFilter === "today") {
        matchesDate = diff <= ONE_DAY;
      } else if (dateFilter === "week") {
        matchesDate = diff <= 7 * ONE_DAY;
      } else if (dateFilter === "month") {
        matchesDate = diff <= 30 * ONE_DAY;
      }
    }

    return matchesSearch && matchesType && matchesDate;
  });

  const clearFilters = () => {
    setSearchTerm("");
    setTypeFilter("all");
    setDateFilter("all");
  };

  const hasActiveFilters = searchTerm !== "" || typeFilter !== "all" || dateFilter !== "all";

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <Header
        title="Meetings"
        subtitle="Browse, search, and review your archived conversations and lectures"
        actions={
          <Button
            variant="primary"
            size="sm"
            onClick={() => void navigate({ to: "/record" })}
          >
            <Plus className="w-3.5 h-3.5 mr-1" />
            Start Recording
          </Button>
        }
      />

      <div className="p-6 max-w-5xl mx-auto w-full space-y-6 flex-1 overflow-y-auto">
        {/* Controls Bar: Search & Filter */}
        <div className="flex flex-col sm:flex-row items-stretch sm:items-center justify-between gap-3">
          <div className="relative flex-1 max-w-md">
            <Search className="w-4 h-4 text-ink-muted absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none" />
            <input
              ref={searchInputRef}
              type="text"
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search meetings by title or keywords… (Cmd+K)"
              className="w-full h-9 pl-9 pr-3 rounded border border-border bg-surface-elevated text-xs text-ink placeholder:text-ink-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              aria-label="Search meetings"
            />
          </div>

          <div className="flex items-center gap-2">
            {/* Classification Filter */}
            <div className="flex items-center gap-1.5">
              <Filter className="w-3.5 h-3.5 text-ink-muted shrink-0" />
              <select
                value={typeFilter}
                onChange={(e) => setTypeFilter(e.target.value)}
                aria-label="Filter meetings by classification"
                className="h-9 px-2.5 rounded border border-border bg-surface-elevated text-xs text-ink font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <option value="all">All Classifications</option>
                <option value="meeting">Business Meeting</option>
                <option value="lecture">Lecture / Academic</option>
                <option value="auto">Auto-detected</option>
              </select>
            </div>

            {/* Date Range Filter */}
            <div className="flex items-center gap-1.5">
              <Calendar className="w-3.5 h-3.5 text-ink-muted shrink-0" />
              <select
                value={dateFilter}
                onChange={(e) => setDateFilter(e.target.value)}
                aria-label="Filter meetings by date range"
                className="h-9 px-2.5 rounded border border-border bg-surface-elevated text-xs text-ink font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <option value="all">All Time</option>
                <option value="today">Today</option>
                <option value="week">Past 7 Days</option>
                <option value="month">Past 30 Days</option>
              </select>
            </div>
          </div>
        </div>

        {/* Empty State */}
        {filteredMeetings.length === 0 ? (
          <EmptyState
            icon={<FolderKanban className="w-6 h-6" />}
            title={hasActiveFilters ? "No matching meetings found" : "No recordings yet"}
            description={
              hasActiveFilters
                ? "Try adjusting your search terms or filter selection."
                : "Your recorded conversations and study sessions will appear here with synchronized video, transcripts, and AI summaries."
            }
            actionLabel={hasActiveFilters ? "Clear Search & Filters" : "Start a Recording"}
            onAction={
              hasActiveFilters ? clearFilters : () => void navigate({ to: "/record" })
            }
          />
        ) : (
          /* Meeting List */
          <div className="space-y-2.5" role="feed" aria-label="Meeting recordings list">
            {filteredMeetings.map((meeting) => (
              <article
                key={meeting.id}
                tabIndex={0}
                role="article"
                aria-labelledby={`meeting-title-${meeting.id}`}
                onClick={() => void navigate({ to: "/meeting/$id", params: { id: meeting.id } })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void navigate({ to: "/meeting/$id", params: { id: meeting.id } });
                  }
                }}
                className="group p-4 rounded-lg border border-border bg-surface-elevated hover:bg-surface/60 transition-all duration-fast cursor-pointer flex items-center justify-between gap-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <div className="space-y-2 min-w-0 flex-1">
                  <div className="flex items-center gap-2.5 flex-wrap">
                    <h2
                      id={`meeting-title-${meeting.id}`}
                      className="text-sm font-semibold text-ink group-hover:text-primary transition-colors truncate"
                    >
                      {meeting.title}
                    </h2>
                    <Badge variant="green" size="sm">
                      {meeting.detected_type || meeting.requested_type}
                    </Badge>

                    {/* Processing State Badge */}
                    <span className="inline-flex items-center gap-1 font-mono text-[10px] text-green-text bg-green-tint px-2 py-0.5 rounded-full font-medium">
                      <CheckCircle2 className="w-2.5 h-2.5" />
                      <span>Processed</span>
                    </span>
                  </div>

                  <div className="flex items-center gap-4 text-xs text-ink-muted">
                    <span className="flex items-center gap-1.5">
                      <Clock className="w-3.5 h-3.5" />
                      <time dateTime={meeting.recorded_at}>{formatDate(meeting.recorded_at)}</time>
                    </span>
                    <span className="flex items-center gap-1.5 font-mono">
                      <Video className="w-3.5 h-3.5" />
                      {formatDuration(meeting.duration_seconds)}
                    </span>
                    {meeting.speaker_count && (
                      <span className="flex items-center gap-1.5">
                        <Users className="w-3.5 h-3.5" />
                        {meeting.speaker_count} Speakers
                      </span>
                    )}
                  </div>
                </div>

                <div className="flex items-center gap-2 shrink-0">
                  {/* Delete button (Task M8.08) */}
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setMeetingToDelete(meeting);
                    }}
                    className="p-1.5 rounded text-ink-muted hover:text-status-error hover:bg-red-50 dark:hover:bg-red-950/20 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    aria-label={`Delete meeting ${meeting.title}`}
                    title="Delete meeting"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>

                  <ChevronRight className="w-4 h-4 text-ink-muted group-hover:text-primary transition-colors" />
                </div>
              </article>
            ))}
          </div>
        )}
      </div>

      {/* Delete Meeting Confirmation Modal */}
      {meetingToDelete && (
        <DeleteMeetingModal
          isOpen={true}
          meeting={meetingToDelete}
          onClose={() => setMeetingToDelete(null)}
          onDeleted={() => {
            void queryClient.invalidateQueries({ queryKey: ["meetings"] });
          }}
        />
      )}
    </div>
  );
};

export const MeetingsView: React.FC = () => {
  return (
    <ErrorBoundary fallbackTitle="Unable to load Meeting Library">
      <MeetingsContent />
    </ErrorBoundary>
  );
};

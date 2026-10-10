import { useUiStore } from "../stores/uiStore";
import { hiddenSidebarInset } from "../lib/platform";
import React, { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Skeleton } from "../components/ui/Skeleton";
import { EmptyState } from "../components/ui/EmptyState";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { MediaPlayer } from "../components/player/MediaPlayer";
import { TranscriptView } from "../components/transcript/TranscriptView";
import { SummaryTab } from "../components/summary/SummaryTab";
import { ActionItemsTab } from "../components/actions/ActionItemsTab";
import { SlidesTab, SlidesStrip } from "../components/slides/SlidesTab";
import { PipelineStatusBadge } from "../components/pipeline/PipelineStatusBadge";
import { ExportModal } from "../components/export/ExportModal";
import {
  getMeeting,
  getMeetingMedia,
  isTauriEnvironment,
  getPipelineStatus,
  listTranscriptSegments,
  getSlides,
  listSummaryRevisions,
  listActionItems,
  createKnowledgeThread,
  listKnowledgeMessages,
  sendKnowledgeMessage,
  exportMeetingAsMarkdown,
  type MeetingDTO,
  type MeetingMediaDTO,
  type TranscriptSegmentDTO,
  type SlideDTO,
  type SummaryRevisionDTO,
  type ActionItemDTO,
  type PipelineStatusDTO,
  type KnowledgeMessageDTO,
} from "../lib/tauri";
import {
  Clock,
  Users,
  CheckSquare,
  FileText,
  Images,
  MessageSquare,
  AlertTriangle,
  ChevronLeft,
  Copy,
  Download,
  Send,
  Check,
} from "lucide-react";

export const MeetingDetailContent: React.FC = () => {
  const sidebarHidden = useUiStore((s) => s.sidebarCollapsed);
  const params = useParams({ strict: false }) as { id?: string };
  const meetingId = params.id || "m-01";
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  // Active Tab: summary, transcript, actions, slides, chat
  const [activeTab, setActiveTab] = useState<"summary" | "transcript" | "actions" | "slides" | "chat">("summary");

  // Synchronized Media Clock
  const [currentTime, setCurrentTime] = useState(0);
  const [isExportOpen, setIsExportOpen] = useState(false);
  const [showCopyToast, setShowCopyToast] = useState(false);
  const [selectedRevisionId, setSelectedRevisionId] = useState<string | null>(null);

  // Queries
  const { data: meeting, isLoading: isMeetingLoading, isError: isMeetingError } = useQuery<MeetingDTO | null>({
    queryKey: ["meeting", meetingId],
    queryFn: () => getMeeting(meetingId),
  });

  const { data: media } = useQuery<MeetingMediaDTO | null>({
    queryKey: ["meeting-media", meetingId],
    queryFn: () => getMeetingMedia(meetingId),
  });

  const { data: pipelineStatus = [] } = useQuery<PipelineStatusDTO[]>({
    queryKey: ["pipeline", meetingId],
    queryFn: () => getPipelineStatus(meetingId),
  });

  const { data: segments = [] } = useQuery<TranscriptSegmentDTO[]>({
    queryKey: ["transcript", meetingId],
    queryFn: () => listTranscriptSegments(meetingId),
  });

  const { data: slides = [] } = useQuery<SlideDTO[]>({
    queryKey: ["slides", meetingId],
    queryFn: () => getSlides(meetingId),
  });

  const { data: revisions = [] } = useQuery<SummaryRevisionDTO[]>({
    queryKey: ["summary_revisions", meetingId],
    queryFn: () => listSummaryRevisions(meetingId),
  });

  // Current Summary Revision
  const activeRevision =
    revisions.find((r) => r.id === selectedRevisionId) || revisions[0] || null;

  const { data: actionItems = [] } = useQuery<ActionItemDTO[]>({
    queryKey: ["action_items", activeRevision?.id],
    queryFn: () => (activeRevision ? listActionItems(activeRevision.id) : Promise.resolve([])),
    enabled: !!activeRevision,
  });

  // Chat Drawer State (In-meeting scoped)
  const [chatInput, setChatInput] = useState("");
  const [chatThreadId, setChatThreadId] = useState<string | null>(null);
  const [chatSending, setChatSending] = useState(false);
  const [chatError, setChatError] = useState<string | null>(null);
  const { data: chatMessages = [] } = useQuery<KnowledgeMessageDTO[]>({
    queryKey: ["meetingKnowledgeMessages", chatThreadId],
    queryFn: () => listKnowledgeMessages(chatThreadId as string),
    enabled: Boolean(chatThreadId),
  });

  useEffect(() => {
    let mounted = true;
    void createKnowledgeThread("this_meeting", meetingId).then((thread) => {
      if (mounted) setChatThreadId(thread.id);
    }).catch((error) => {
      if (mounted) setChatError(error instanceof Error ? error.message : "Meeting chat is unavailable.");
    });
    return () => { mounted = false; };
  }, [meetingId]);

  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  // Seek handler called by player, transcript, slides, citations
  const handleSeek = useCallback((seconds: number) => {
    setCurrentTime(seconds);
  }, []);

  // Quick clipboard copy
  const handleQuickCopy = async () => {
    if (!meeting || !activeRevision) return;
    const md = exportMeetingAsMarkdown(meeting, activeRevision, actionItems, segments);
    try {
      await navigator.clipboard.writeText(md);
      setShowCopyToast(true);
      setTimeout(() => setShowCopyToast(false), 2000);
    } catch {
      // Fallback
    }
  };

  // Keyboard shortcut 'C' for copy summary
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
        return;
      }
      if (e.key === "c" || e.key === "C") {
        if (!e.metaKey && !e.ctrlKey) {
          e.preventDefault();
          void handleQuickCopy();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [meeting, activeRevision, actionItems, segments]);

  const handleSendChat = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!chatInput.trim() || !chatThreadId || chatSending) return;
    const userMsg = chatInput.trim();
    setChatInput("");
    setChatError(null);
    setChatSending(true);
    try {
      await sendKnowledgeMessage(chatThreadId, userMsg);
      await queryClient.invalidateQueries({ queryKey: ["meetingKnowledgeMessages", chatThreadId] });
    } catch (error) {
      setChatError(error instanceof Error ? error.message : "The selected provider could not answer.");
    } finally {
      setChatSending(false);
    }
  };

  if (isMeetingLoading) {
    return (
      <div className="p-6 max-w-7xl mx-auto w-full space-y-4">
        <Skeleton className="h-8 w-64" />
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <Skeleton className="h-96 w-full" />
          <Skeleton className="h-96 w-full" />
        </div>
      </div>
    );
  }

  if (isMeetingError || !meeting) {
    return (
      <EmptyState
        icon={<AlertTriangle className="w-8 h-8 text-status-warning" />}
        title="Meeting Record Not Found"
        description="The requested meeting could not be located in your local storage catalog."
        actionLabel="Back to Meetings"
        onAction={() => void navigate({ to: "/" })}
      />
    );
  }

  // Audio-only check: if meeting requested/detected type has no screen source
  // In the desktop app the backend is authoritative; the fixture fallback
  // (m-02 is audio-only) only applies to browser/mock mode.
  const hasVideo = media ? media.has_video : isTauriEnvironment() ? false : meeting.id !== "m-02";
  const pendingActionsCount = actionItems.filter((a) => !a.completed).length;

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-bg">
      {/* Precision Master Control Studio Header Bar (DESIGN.md §4.2) */}
      <div className={`px-6 pt-4 pb-3 ${hiddenSidebarInset(sidebarHidden)}`}>
        <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
          <div className="flex items-center gap-3 min-w-0 flex-1 basis-[260px]">
            <button
              type="button"
              onClick={() => void navigate({ to: "/" })}
              className="p-1.5 rounded-full text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              aria-label="Back to meeting library"
            >
              <ChevronLeft className="w-5 h-5" />
            </button>
            <div className="min-w-0">
              <h1 className="text-base font-semibold text-ink tracking-tight flex items-center gap-2 min-w-0">
                <span className="truncate" title={meeting.title}>{meeting.title}</span>
                <Badge variant="green" size="sm" className="shrink-0">
                  {meeting.detected_type || meeting.requested_type}
                </Badge>
              </h1>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-ink-muted mt-0.5 whitespace-nowrap">
                <span className="flex items-center gap-1 font-mono">
                  <Clock className="w-3 h-3" />
                  {formatTime(meeting.duration_seconds)}
                </span>
                <span>·</span>
                <span className="flex items-center gap-1">
                  <Users className="w-3 h-3" />
                  {meeting.speaker_count || 3} Speakers
                </span>
                <span>·</span>
                <time dateTime={meeting.recorded_at}>
                  {new Date(meeting.recorded_at).toLocaleDateString("en-US", {
                    month: "short",
                    day: "numeric",
                    year: "numeric",
                  })}
                </time>
              </div>
            </div>
          </div>

          {/* Header Action Tools */}
          <div className="flex items-center gap-2.5 shrink-0">
            {/* Pipeline Status Trigger Badge (Task M8.07) */}
            <PipelineStatusBadge
              statusList={pipelineStatus}
              onRetryStep={() => {
                void queryClient.invalidateQueries({ queryKey: ["pipeline", meetingId] });
              }}
            />

            {/* Quick Copy Button (Task M8.09) */}
            <Button variant="secondary" size="sm" onClick={handleQuickCopy} title="Copy summary (C)">
              {showCopyToast ? (
                <>
                  <Check className="w-3.5 h-3.5 mr-1 text-primary" />
                  Copied!
                </>
              ) : (
                <>
                  <Copy className="w-3.5 h-3.5 mr-1" />
                  Copy
                </>
              )}
            </Button>

            {/* Export Dialog Button (Task M8.09) */}
            <Button variant="secondary" size="sm" onClick={() => setIsExportOpen(true)}>
              <Download className="w-3.5 h-3.5 mr-1" />
              Export
            </Button>
          </div>
        </div>
      </div>

      {/* Main Split View: Media Player & Slide Strip (Left) | Tabs Studio (Right) */}
      <div className="flex-1 min-h-0 grid grid-cols-1 lg:grid-cols-12 auto-rows-min lg:auto-rows-auto gap-4 px-6 pb-6 overflow-y-auto lg:overflow-hidden">
        {/* Left Column (5/12 cols): Video Player + Auto-extracted Slides Strip */}
        <div className="lg:col-span-6 xl:col-span-5 p-4 rounded-2xl bg-surface flex flex-col gap-4 lg:min-h-0 lg:overflow-y-auto">
          {/* Synchronized Media Player (Task M8.01) */}
          <MediaPlayer
            src={media?.path ?? undefined}
            hasVideo={hasVideo}
            meetingTitle={meeting.title}
            durationSeconds={meeting.duration_seconds}
            currentTime={currentTime}
            onTimeUpdate={setCurrentTime}
            onSeek={handleSeek}
          />

          {/* Auto-extracted Slides Strip (Task M8.06) */}
          <SlidesStrip
            slides={slides}
            hasVideo={hasVideo}
            currentTime={currentTime}
            onSeek={handleSeek}
            onViewAll={() => setActiveTab("slides")}
          />
        </div>

        {/* Right Column (7/12 cols): Tabbed Studio Workspace */}
        <div className="lg:col-span-6 xl:col-span-7 flex flex-col min-h-[34rem] lg:min-h-0 rounded-2xl bg-surface">
          {/* Tab Navigation Header (DESIGN.md §4.2) */}
          <div className="h-14 flex items-center px-4 gap-1 shrink-0 overflow-x-auto">
            <button
              type="button"
              onClick={() => setActiveTab("summary")}
              className={`h-8 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                activeTab === "summary"
                  ? "bg-surface-hover text-ink font-semibold"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              <FileText className="w-3.5 h-3.5 lg:max-xl:hidden" />
              Summary
            </button>

            <button
              type="button"
              onClick={() => setActiveTab("transcript")}
              className={`h-8 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                activeTab === "transcript"
                  ? "bg-surface-hover text-ink font-semibold"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              <Clock className="w-3.5 h-3.5 lg:max-xl:hidden" />
              Transcript
            </button>

            <button
              type="button"
              onClick={() => setActiveTab("actions")}
              className={`h-8 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                activeTab === "actions"
                  ? "bg-surface-hover text-ink font-semibold"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              <CheckSquare className="w-3.5 h-3.5 lg:max-xl:hidden" />
              <span>Action Items</span>
              {pendingActionsCount > 0 && (
                <span className="ml-1 px-1.5 py-0.2 rounded-full bg-primary text-on-primary text-[10px] font-mono font-bold">
                  {pendingActionsCount}
                </span>
              )}
            </button>

            <button
              type="button"
              onClick={() => setActiveTab("slides")}
              className={`h-8 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                activeTab === "slides"
                  ? "bg-surface-hover text-ink font-semibold"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              <Images className="w-3.5 h-3.5 lg:max-xl:hidden" />
              <span>Slides</span>
              {slides.length > 0 && hasVideo && (
                <span className="ml-1 text-[10px] font-mono text-ink-muted">({slides.length})</span>
              )}
            </button>

            <button
              type="button"
              onClick={() => setActiveTab("chat")}
              className={`h-8 px-3 rounded-full text-xs font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                activeTab === "chat"
                  ? "bg-surface-hover text-ink font-semibold"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              <MessageSquare className="w-3.5 h-3.5 lg:max-xl:hidden" />
              Chat
            </button>
          </div>

          {/* Tab Content Body */}
          <div className="flex-1 p-6 overflow-y-auto">
            {/* TAB: SUMMARY (Task M8.04) */}
            {activeTab === "summary" && activeRevision && (
              <SummaryTab
                meetingId={meeting.id}
                summary={activeRevision}
                revisions={revisions}
                onSelectRevision={(rev) => setSelectedRevisionId(rev.id)}
                onSeek={handleSeek}
                onRegenerated={() => {
                  void queryClient.invalidateQueries({ queryKey: ["summary_revisions", meetingId] });
                }}
              />
            )}

            {/* TAB: TRANSCRIPT (Tasks M8.02 & M8.03) */}
            {activeTab === "transcript" && (
              <TranscriptView
                segments={segments}
                currentTime={currentTime}
                onSeek={handleSeek}
                onSpeakerRenamed={() => {
                  void queryClient.invalidateQueries({ queryKey: ["transcript", meetingId] });
                }}
              />
            )}

            {/* TAB: ACTION ITEMS (Task M8.05) */}
            {activeTab === "actions" && activeRevision && (
              <ActionItemsTab
                summaryRevisionId={activeRevision.id}
                actionItems={actionItems}
                onToggleAction={() => {
                  void queryClient.invalidateQueries({ queryKey: ["action_items", activeRevision.id] });
                }}
              />
            )}

            {/* TAB: SLIDES (Task M8.06) */}
            {activeTab === "slides" && (
              <SlidesTab
                slides={slides}
                hasVideo={hasVideo}
                currentTime={currentTime}
                onSeek={handleSeek}
                onRetryOcr={() => {
                  void queryClient.invalidateQueries({ queryKey: ["slides", meetingId] });
                }}
              />
            )}

            {/* TAB: IN-MEETING CHAT DRAWER */}
            {activeTab === "chat" && (
              <div className="flex flex-col h-full space-y-4" role="region" aria-label="In-meeting scoped Q&A">
                <div className="p-3 rounded-xl bg-green-tint/60 text-xs text-green-text flex items-center justify-between">
                  <span>Scoped strictly to this meeting’s transcript, slides, and summary.</span>
                  <span className="font-mono text-[10px]">Llama 3.2 3B · Local (Private)</span>
                </div>

                {chatError && <p role="alert" className="text-xs text-status-error">{chatError}</p>}
                <div className="flex-1 space-y-3 overflow-y-auto pr-1">
                  {chatMessages.map((msg) => (
                    <div key={msg.id} className={"p-3 rounded-2xl text-xs leading-relaxed max-w-[85%] " + (msg.role === "user" ? "ml-auto bg-primary text-on-primary font-medium" : "mr-auto bg-bg text-ink")}>
                      <p className="whitespace-pre-wrap">{msg.content}</p>
                      {msg.citations.map((citation) => (
                        <button key={citation.id} type="button" onClick={() => citation.start_seconds !== null && handleSeek(citation.start_seconds)} className="block mt-1.5 text-[10px] font-mono text-primary border-t border-border/40 pt-1 hover:underline">
                          Citation: {citation.source_title}{citation.start_seconds !== null ? " · " + formatTime(citation.start_seconds) : citation.page_number !== null ? " · page " + citation.page_number : ""}
                        </button>
                      ))}
                    </div>
                  ))}
                </div>

                <form onSubmit={handleSendChat} className="flex items-center gap-2 pt-2">
                  <input
                    type="text"
                    value={chatInput}
                    onChange={(e) => setChatInput(e.target.value)}
                    placeholder="Ask about this meeting…"
                    className="flex-1 h-10 px-4 rounded-full bg-bg text-xs text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  />
                  <Button variant="primary" size="sm" type="submit" disabled={chatSending}>
                    <Send className="w-3.5 h-3.5 mr-1" />
                    {chatSending ? "Thinking…" : "Ask"}
                  </Button>
                </form>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Export Dialog Modal (Task M8.09) */}
      {activeRevision && (
        <ExportModal
          isOpen={isExportOpen}
          onClose={() => setIsExportOpen(false)}
          meeting={meeting}
          summary={activeRevision}
          actionItems={actionItems}
          segments={segments}
          slides={slides}
        />
      )}
    </div>
  );
};

export const MeetingDetailView: React.FC = () => {
  return (
    <ErrorBoundary fallbackTitle="Unable to load Meeting Detail Studio">
      <MeetingDetailContent />
    </ErrorBoundary>
  );
};

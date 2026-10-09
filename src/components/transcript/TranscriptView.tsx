import React, { useRef, useState, useEffect, useCallback } from "react";
import {
  type TranscriptSegmentDTO,
  getSpeakerPalette,
  renameSpeaker,
} from "../../lib/tauri";
import { useUiStore } from "../../stores/uiStore";
import { Search, ArrowDownCircle, Edit2, Check, X } from "lucide-react";

export interface TranscriptViewProps {
  segments: TranscriptSegmentDTO[];
  currentTime: number;
  onSeek: (time: number) => void;
  onSpeakerRenamed?: (speakerId: string, newName: string) => void;
}

export const TranscriptView: React.FC<TranscriptViewProps> = ({
  segments,
  currentTime,
  onSeek,
  onSpeakerRenamed,
}) => {
  const theme = useUiStore((state) => state.theme);
  const isDark = theme === "dark";

  const containerRef = useRef<HTMLDivElement | null>(null);
  const activeSegmentRef = useRef<HTMLDivElement | null>(null);

  const [filterText, setFilterText] = useState("");
  const [isUserScrolling, setIsUserScrolling] = useState(false);
  const scrollTimeoutRef = useRef<NodeJS.Timeout | null>(null);

  // Speaker renaming modal/dialog state
  const [editingSpeakerId, setEditingSpeakerId] = useState<string | null>(null);
  const [editingSpeakerName, setEditingSpeakerName] = useState("");

  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  // Find active segment index
  const activeIndex = segments.findIndex(
    (seg) => currentTime >= seg.start_time && currentTime <= seg.end_time
  );

  // Auto-scroll to center active segment (DESIGN.md §4.2.C)
  const scrollToActive = useCallback(() => {
    if (activeSegmentRef.current && containerRef.current) {
      activeSegmentRef.current.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    }
  }, []);

  useEffect(() => {
    if (!isUserScrolling && activeIndex !== -1) {
      scrollToActive();
    }
  }, [activeIndex, isUserScrolling, scrollToActive]);

  // Handle manual scroll pause (pause for 8s)
  const handleScroll = () => {
    setIsUserScrolling(true);
    if (scrollTimeoutRef.current) {
      clearTimeout(scrollTimeoutRef.current);
    }
    scrollTimeoutRef.current = setTimeout(() => {
      setIsUserScrolling(false);
    }, 8000);
  };

  // Jump to current pill click
  const handleJumpToCurrent = () => {
    setIsUserScrolling(false);
    if (scrollTimeoutRef.current) {
      clearTimeout(scrollTimeoutRef.current);
    }
    scrollToActive();
  };

  // Keyboard navigation: J (next), K (previous)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
        return;
      }

      if (e.key === "j" || e.key === "J") {
        e.preventDefault();
        const nextIndex = activeIndex < segments.length - 1 ? activeIndex + 1 : 0;
        if (segments[nextIndex]) {
          onSeek(segments[nextIndex].start_time);
        }
      } else if (e.key === "k" || e.key === "K") {
        e.preventDefault();
        const prevIndex = activeIndex > 0 ? activeIndex - 1 : segments.length - 1;
        if (segments[prevIndex]) {
          onSeek(segments[prevIndex].start_time);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeIndex, segments, onSeek]);

  // Handle speaker rename submit
  const handleSaveSpeakerName = async (speakerId: string) => {
    if (!editingSpeakerName.trim()) return;
    try {
      await renameSpeaker(speakerId, editingSpeakerName.trim());
      onSpeakerRenamed?.(speakerId, editingSpeakerName.trim());
    } finally {
      setEditingSpeakerId(null);
      setEditingSpeakerName("");
    }
  };

  const filteredSegments = segments.filter((seg) => {
    if (!filterText.trim()) return true;
    const q = filterText.toLowerCase();
    return (
      seg.text.toLowerCase().includes(q) ||
      seg.speaker_label.toLowerCase().includes(q) ||
      formatTime(seg.start_time).includes(q)
    );
  });

  return (
    <div className="relative flex flex-col h-full min-h-0">
      {/* Transcript Toolbar: Search & Filter */}
      <div className="pb-3 flex items-center justify-between gap-3">
        <div className="relative flex-1 max-w-sm">
          <Search className="w-3.5 h-3.5 text-ink-muted absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            type="text"
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
            placeholder="Search transcript text or speaker… (J/K to step)"
            className="w-full h-8 pl-8 pr-2.5 rounded-full bg-bg text-xs text-ink placeholder:text-ink-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            aria-label="Filter transcript"
          />
        </div>

        <div className="text-[11px] font-mono text-ink-muted shrink-0">
          <span>{filteredSegments.length} segments</span>
        </div>
      </div>

      {/* Floating "Jump to current" Pill Button (DESIGN.md §4.2.C) */}
      {isUserScrolling && activeIndex !== -1 && (
        <button
          type="button"
          onClick={handleJumpToCurrent}
          className="absolute bottom-4 right-6 z-20 px-3 py-1.5 rounded-full bg-primary text-on-primary text-xs font-semibold shadow-lg hover:bg-primary-hover flex items-center gap-1.5 transition-all duration-fast animate-bounce"
          aria-label="Jump to currently playing segment"
        >
          <ArrowDownCircle className="w-3.5 h-3.5" />
          <span>Jump to current ({formatTime(currentTime)})</span>
        </button>
      )}

      {/* Virtualized / Scrollable Transcript Feed */}
      <div
        ref={containerRef}
        onScroll={handleScroll}
        role="feed"
        aria-label="Synchronized meeting transcript"
        className="flex-1 overflow-y-auto p-4 space-y-2.5 focus-visible:outline-none"
      >
        {filteredSegments.length === 0 ? (
          <div className="py-12 text-center text-xs text-ink-muted font-normal">
            No matching spoken segments found.
          </div>
        ) : (
          filteredSegments.map((seg, idx) => {
            const isActive = currentTime >= seg.start_time && currentTime <= seg.end_time;
            const palette = getSpeakerPalette(seg.speaker_id || seg.speaker_label, isDark);

            return (
              <div
                key={seg.id}
                ref={isActive ? activeSegmentRef : null}
                onClick={() => onSeek(seg.start_time)}
                className={`group relative p-3 rounded-lg border transition-all duration-fast cursor-pointer text-left ${
                  isActive
                    ? "bg-surface border-border shadow-xs pl-4.5 ring-1 ring-mint/40"
                    : "bg-transparent border-transparent hover:bg-surface/50"
                }`}
                role="article"
                aria-current={isActive ? "time" : undefined}
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onSeek(seg.start_time);
                  }
                }}
              >
                {/* 3px Mint Accent Bar along Left Edge for Active Segment (DESIGN.md §2.1 & §4.2.C) */}
                {isActive && (
                  <span
                    className="absolute left-0 top-1.5 bottom-1.5 w-[3px] bg-mint rounded-r"
                    aria-hidden="true"
                  />
                )}

                {/* Metadata Header: Timestamp + Speaker Diarization Badge */}
                <div className="flex items-center justify-between gap-2 mb-1.5">
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onSeek(seg.start_time);
                      }}
                      className="font-mono text-xs text-ink-muted hover:text-primary transition-colors focus-visible:outline-none font-medium"
                      title="Seek to start"
                    >
                      {formatTime(seg.start_time)}
                    </button>

                    {/* Speaker Badge with Diarization Palette Hue (DESIGN.md §2.3) */}
                    <div className="relative inline-flex items-center">
                      <span
                        className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold border transition-colors cursor-pointer"
                        style={{
                          color: palette.text,
                          backgroundColor: palette.bg,
                          borderColor: palette.border,
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          setEditingSpeakerId(seg.speaker_id);
                          setEditingSpeakerName(seg.speaker_label);
                        }}
                        title={`Click to rename speaker (${palette.hue} hue)`}
                      >
                        <span>{seg.speaker_label}</span>
                        <Edit2 className="w-2.5 h-2.5 opacity-60 hover:opacity-100" />
                      </span>
                    </div>
                  </div>

                  <span className="font-mono text-[10px] text-ink-subtle">
                    {formatTime(seg.end_time - seg.start_time)}s
                  </span>
                </div>

                {/* Speaker Dialogue Text */}
                <p className="text-sm text-ink leading-relaxed font-normal">{seg.text}</p>
              </div>
            );
          })
        )}
      </div>

      {/* Speaker Rename Modal / Inline Dialog (DESIGN.md §2.3) */}
      {editingSpeakerId && (
        <div
          role="dialog"
          aria-labelledby="rename-speaker-title"
          className="fixed inset-0 z-50 bg-black/50 backdrop-blur-xs flex items-center justify-center p-4"
          onClick={() => setEditingSpeakerId(null)}
        >
          <div
            className="w-full max-w-sm rounded-2xl border border-border bg-surface-elevated p-5 shadow-xl space-y-4"
            onClick={(e) => e.stopPropagation()}
          >
            <div>
              <h3 id="rename-speaker-title" className="text-sm font-semibold text-ink">
                Rename Speaker
              </h3>
              <p className="text-xs text-ink-muted mt-1">
                Updates structured references across the meeting without modifying verbatim quotes.
              </p>
            </div>

            <input
              type="text"
              autoFocus
              value={editingSpeakerName}
              onChange={(e) => setEditingSpeakerName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  handleSaveSpeakerName(editingSpeakerId);
                } else if (e.key === "Escape") {
                  setEditingSpeakerId(null);
                }
              }}
              placeholder="e.g. Morgan Vance"
              className="w-full h-9 px-3 rounded-lg border border-border bg-surface-sunken text-xs text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              aria-label="New speaker display name"
            />

            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setEditingSpeakerId(null)}
                className="px-3.5 py-1.5 rounded-full text-xs text-ink-muted hover:text-ink hover:bg-surface-hover transition-colors"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => handleSaveSpeakerName(editingSpeakerId)}
                className="px-3.5 py-1.5 rounded-full bg-primary text-on-primary text-xs font-semibold hover:bg-primary-hover transition-colors"
              >
                Save Name
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

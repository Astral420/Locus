import React, { useState } from "react";
import { type SummaryRevisionDTO, triggerSummary } from "../../lib/tauri";
import {
  AlertTriangle,
  RotateCw,
  History,
  CheckCircle2,
  Clock,
  Sparkles,
  Download,
  Settings,
} from "lucide-react";
import { Button } from "../ui/Button";
import { Badge } from "../ui/Badge";

export interface SummaryTabProps {
  meetingId: string;
  summary: SummaryRevisionDTO;
  revisions: SummaryRevisionDTO[];
  onSelectRevision: (revision: SummaryRevisionDTO) => void;
  onSeek: (seconds: number) => void;
  onRegenerated?: () => void;
}

// Parses string and renders timestamp citations like [14:32] as interactive clickable badges
export const renderWithCitations = (
  text: string,
  onSeek: (seconds: number) => void
): React.ReactNode => {
  const parts: React.ReactNode[] = [];
  const citationRegex = /\[(\d{1,2}):(\d{2})\]/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = citationRegex.exec(text)) !== null) {
    const start = match.index;
    const end = citationRegex.lastIndex;

    // Add preceding text
    if (start > lastIndex) {
      parts.push(text.slice(lastIndex, start));
    }

    const minutes = parseInt(match[1], 10);
    const seconds = parseInt(match[2], 10);
    const totalSeconds = minutes * 60 + seconds;
    const citationLabel = match[0];

    parts.push(
      <button
        key={`${start}-${end}`}
        type="button"
        onClick={() => onSeek(totalSeconds)}
        className="inline-flex items-center mx-1 px-1.5 py-0.2 rounded bg-green-tint text-green-text font-mono text-[11px] font-semibold hover:bg-primary hover:text-white transition-colors cursor-pointer border border-primary/20 align-baseline"
        title={`Seek to ${citationLabel}`}
        aria-label={`Jump to citation ${citationLabel}`}
      >
        {citationLabel}
      </button>
    );

    lastIndex = end;
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return parts;
};

export const SummaryTab: React.FC<SummaryTabProps> = ({
  meetingId,
  summary,
  revisions,
  onSelectRevision,
  onSeek,
  onRegenerated,
}) => {
  const [isOutdatedDismissed, setIsOutdatedDismissed] = useState(false);
  const [isRegenerating, setIsRegenerating] = useState(false);

  const showOutdatedBanner = summary.is_outdated && !isOutdatedDismissed;

  const handleRegenerate = async () => {
    setIsRegenerating(true);
    try {
      await triggerSummary(meetingId, true);
      onRegenerated?.();
    } finally {
      setIsRegenerating(false);
    }
  };

  const formatDate = (iso: string) => {
    return new Date(iso).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  return (
    <div className="space-y-6 max-w-3xl pb-8" role="region" aria-label="Meeting AI Summary">
      {/* Revision Bar & Provenance Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 p-3 rounded-lg border border-border bg-surface/40">
        <div className="flex items-center gap-2.5">
          <History className="w-4 h-4 text-ink-muted shrink-0" />
          <div className="flex items-center gap-2">
            <label htmlFor="revision-selector" className="text-xs font-medium text-ink-muted">
              Revision:
            </label>
            <select
              id="revision-selector"
              value={summary.id}
              onChange={(e) => {
                const target = revisions.find((r) => r.id === e.target.value);
                if (target) onSelectRevision(target);
              }}
              className="h-8 px-2.5 rounded border border-border bg-surface-elevated text-xs font-semibold text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              aria-label="Select summary revision"
            >
              {revisions.map((rev) => (
                <option key={rev.id} value={rev.id}>
                  v{rev.revision_number} {rev.is_outdated ? "(Outdated)" : "(Current)"} ·{" "}
                  {formatDate(rev.created_at)}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="flex items-center gap-2 text-xs text-ink-muted font-mono">
          <span className="flex items-center gap-1">
            <Sparkles className="w-3.5 h-3.5 text-primary" />
            <span>Model: {summary.model}</span>
          </span>
          <span>·</span>
          <span>{formatDate(summary.created_at)}</span>
        </div>
      </div>

      {/* Outdated Warning Banner (DESIGN.md §4.6.2 & PRD FR12.6) */}
      {showOutdatedBanner && (
        <div
          role="alert"
          className="p-3.5 rounded-lg border border-status-warning/40 bg-amber-50/60 dark:bg-amber-950/20 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs"
        >
          <div className="flex items-start sm:items-center gap-2.5 text-amber-900 dark:text-amber-200">
            <AlertTriangle className="w-4 h-4 text-status-warning shrink-0 mt-0.5 sm:mt-0" />
            <div>
              <span className="font-semibold">Source content was updated</span>
              <span className="text-amber-800/80 dark:text-amber-300/80 ml-1">
                — Current summary revision is outdated. Previous completed action items are preserved.
              </span>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              onClick={() => setIsOutdatedDismissed(true)}
              className="px-2.5 py-1 rounded text-ink-muted hover:text-ink hover:bg-surface text-xs transition-colors"
            >
              Keep Current
            </button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleRegenerate}
              disabled={isRegenerating}
            >
              <RotateCw className={`w-3 h-3 mr-1 ${isRegenerating ? "animate-spin" : ""}`} />
              {isRegenerating ? "Regenerating…" : "Regenerate Summary"}
            </Button>
          </div>
        </div>
      )}

      {/* Section 1: Executive Overview */}
      <section className="space-y-2">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-ink-muted flex items-center gap-1.5">
          <span>Executive Overview</span>
        </h2>
        <div className="text-sm text-ink leading-relaxed font-normal bg-surface/20 p-4 rounded-lg border border-border/60">
          <p>{renderWithCitations(summary.overview, onSeek)}</p>
        </div>
      </section>

      {/* Section 2: Key Decisions */}
      <section className="space-y-2.5">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-ink-muted flex items-center gap-1.5">
          <span>Key Decisions & Consensuses</span>
        </h2>
        <div className="space-y-2">
          {summary.decisions.map((decision, idx) => (
            <div
              key={idx}
              className="p-3 rounded-lg border border-border/80 bg-surface-elevated hover:bg-surface/50 transition-colors flex items-start gap-2.5"
            >
              <CheckCircle2 className="w-4 h-4 text-primary shrink-0 mt-0.5" />
              <div className="text-sm text-ink leading-relaxed flex-1">
                {renderWithCitations(decision, onSeek)}
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* Section 3: Key Concepts & Architecture */}
      <section className="space-y-2.5">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-ink-muted flex items-center gap-1.5">
          <span>Key Concepts & Architectural Themes</span>
        </h2>
        <div className="flex flex-wrap gap-2">
          {summary.key_concepts.map((concept, idx) => (
            <span
              key={idx}
              className="px-3 py-1.5 rounded-md bg-surface text-ink text-xs font-medium border border-border hover:border-primary/40 transition-colors"
            >
              {concept}
            </span>
          ))}
        </div>
      </section>
    </div>
  );
};

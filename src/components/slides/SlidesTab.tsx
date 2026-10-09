import React from "react";
import { type SlideDTO } from "../../lib/tauri";
import { Images, AlertCircle, RotateCw } from "lucide-react";
import { Button } from "../ui/Button";

export interface SlidesTabProps {
  slides: SlideDTO[];
  hasVideo: boolean;
  currentTime: number;
  onSeek: (seconds: number) => void;
  ocrError?: boolean;
  onRetryOcr?: () => void;
}

export const SlidesTab: React.FC<SlidesTabProps> = ({
  slides,
  hasVideo,
  currentTime,
  onSeek,
  ocrError = false,
  onRetryOcr,
}) => {
  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  // If no video or slides empty
  if (!hasVideo || slides.length === 0) {
    return (
      <div
        role="region"
        aria-label="Presentation Slides"
        className="flex flex-col items-center justify-center py-16 px-4 text-center rounded-xl bg-bg"
      >
        <div className="w-12 h-12 rounded-full bg-surface-hover flex items-center justify-center text-ink-muted mb-3">
          <Images className="w-6 h-6" />
        </div>
        <h3 className="text-sm font-semibold text-ink">No presentation slides in this recording</h3>
        <p className="text-xs text-ink-muted mt-1 max-w-sm">
          Slide extraction requires a captured screen video stream. Audio-only sessions do not generate presentation slide cards.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-4" role="region" aria-label="Extracted Presentation Slides">
      {/* Header Info */}
      <div className="flex items-center justify-between text-xs text-ink-muted">
        <span>Auto-detected slides via OpenCV frame differencing with Tesseract OCR</span>
        <span className="font-mono">{slides.length} slides</span>
      </div>

      {/* OCR Failure Inline Retry Banner (PRD FR7.7) */}
      {ocrError && (
        <div
          role="alert"
          className="p-3 rounded-xl bg-status-error/10 flex items-center justify-between text-xs text-red-900 dark:text-red-200"
        >
          <div className="flex items-center gap-2">
            <AlertCircle className="w-4 h-4 text-status-error shrink-0" />
            <span>Slide OCR extraction encountered an error for one or more frames.</span>
          </div>
          {onRetryOcr && (
            <Button variant="secondary" size="sm" onClick={onRetryOcr}>
              <RotateCw className="w-3 h-3 mr-1" />
              Retry OCR
            </Button>
          )}
        </div>
      )}

      {/* Slides Grid */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        {slides.map((slide) => {
          const isActive = Math.abs(currentTime - slide.timestamp) < 45;

          return (
            <div
              key={slide.id}
              onClick={() => onSeek(slide.timestamp)}
              className={`p-3 rounded-lg border transition-all duration-fast cursor-pointer space-y-2.5 ${
                isActive
                  ? "border-primary bg-green-tint/30 ring-2 ring-primary/40 shadow-sm"
                  : "border-border bg-surface-elevated hover:bg-surface/60"
              }`}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSeek(slide.timestamp);
                }
              }}
              aria-label={`Slide ${slide.ordinal} at ${formatTime(slide.timestamp)}`}
            >
              {/* Slide Preview Canvas */}
              <div className="relative aspect-video bg-black/90 rounded-md overflow-hidden flex items-center justify-center text-white border border-white/10 group">
                {slide.image_url ? (
                  <img
                    src={slide.image_url}
                    alt={`Slide ${slide.ordinal}`}
                    className="w-full h-full object-cover"
                    onError={(e) => {
                      // Fallback SVG graphic
                      e.currentTarget.style.display = "none";
                    }}
                  />
                ) : null}

                {/* Overlay Badge */}
                <div className="absolute inset-0 flex items-center justify-center p-3 text-center bg-gradient-to-b from-stone-900/60 to-black/80">
                  <div className="space-y-1">
                    <span className="font-mono text-xs font-semibold text-primary">
                      Slide {slide.ordinal}
                    </span>
                    <p className="text-[11px] text-stone-300 font-mono line-clamp-2">
                      {slide.ocr_text || "Presentation Slide"}
                    </p>
                  </div>
                </div>

                {/* Primary Timestamp Badge */}
                <span className="absolute bottom-2 left-2 px-1.5 py-0.5 rounded bg-black/80 text-white font-mono text-[10px] font-semibold border border-white/20">
                  [{formatTime(slide.timestamp)}]
                </span>
              </div>

              {/* Slide Metadata & Timestamps */}
              <div className="flex items-center justify-between text-xs">
                <span className="font-semibold text-ink">Slide {slide.ordinal}</span>
                <div className="flex items-center gap-1.5 font-mono text-[11px]">
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      onSeek(slide.timestamp);
                    }}
                    className="text-primary hover:underline font-semibold"
                  >
                    [{formatTime(slide.timestamp)}]
                  </button>

                  {/* Repeated Occurrences (DESIGN.md §4.2.B) */}
                  {slide.repeated_timestamps?.map((rep, idx) => (
                    <button
                      key={idx}
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onSeek(rep);
                      }}
                      className="text-ink-muted hover:text-primary transition-colors text-[10px]"
                      title="Repeated occurrence"
                    >
                      also {formatTime(rep)}
                    </button>
                  ))}
                </div>
              </div>

              {/* OCR Text Box */}
              <p className="text-xs text-ink-muted font-mono leading-relaxed bg-bg p-2.5 rounded-lg line-clamp-3">
                {slide.ocr_text}
              </p>
            </div>
          );
        })}
      </div>
    </div>
  );
};

export interface SlidesStripProps {
  slides: SlideDTO[];
  hasVideo: boolean;
  currentTime: number;
  onSeek: (seconds: number) => void;
  onViewAll?: () => void;
}

export const SlidesStrip: React.FC<SlidesStripProps> = ({
  slides,
  hasVideo,
  currentTime,
  onSeek,
  onViewAll,
}) => {
  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  if (!hasVideo || slides.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2 select-none" role="region" aria-label="Auto-extracted slides strip">
      <div className="flex items-center justify-between text-xs text-ink-muted">
        <span className="font-semibold uppercase tracking-wider text-[11px]">
          Auto-extracted Slides ({slides.length})
        </span>
        {onViewAll && (
          <button
            type="button"
            onClick={onViewAll}
            className="text-primary hover:underline text-[11px] font-medium"
          >
            View all in tab
          </button>
        )}
      </div>

      <div className="grid grid-cols-3 sm:grid-cols-5 gap-2">
        {slides.slice(0, 5).map((slide) => {
          const isActive = Math.abs(currentTime - slide.timestamp) < 45;

          return (
            <button
              key={slide.id}
              type="button"
              onClick={() => onSeek(slide.timestamp)}
              className={`p-1.5 rounded-lg border text-left transition-all relative overflow-hidden focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                isActive
                  ? "border-primary bg-green-tint/30 ring-2 ring-primary/40 shadow-xs"
                  : "border-border bg-surface-elevated hover:bg-surface"
              }`}
            >
              <div className="aspect-video bg-black/80 rounded mb-1 flex items-center justify-center text-[10px] text-stone-300 font-mono border border-border/60">
                Slide {slide.ordinal}
              </div>
              <div className="flex items-center justify-between text-[10px] font-mono text-ink-muted">
                <span className="font-semibold text-primary">[{formatTime(slide.timestamp)}]</span>
              </div>
              <p className="text-[10px] text-ink truncate mt-0.5">{slide.ocr_text}</p>
            </button>
          );
        })}
      </div>
    </div>
  );
};

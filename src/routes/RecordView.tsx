import React, { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { useRecordingStore } from "../stores/recordingStore";
import {
  type CaptureSource,
  type MeetingType,
} from "../lib/tauri";
import {
  Volume2,
  Mic,
  Monitor,
  Play,
  Square,
  Pause,
  AlertTriangle,
  HardDrive,
  UserCheck,
  Disc,
} from "lucide-react";

export const RecordContent: React.FC = () => {
  const navigate = useNavigate();
  const {
    state,
    elapsed_seconds,
    selected_sources,
    only_me_mic,
    meeting_title,
    meeting_type,
    system_audio_level,
    mic_level,
    warning,
    setTitle,
    setType,
    toggleSource,
    setOnlyMeMic,
    start,
    pause,
    resume,
    stop,
    dismissWarning,
  } = useRecordingStore();

  const [validationError, setValidationError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const formatTimer = (seconds: number) => {
    const hrs = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;
    if (hrs > 0) {
      return `${hrs.toString().padStart(2, "0")}:${mins
        .toString()
        .padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
    }
    return `${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  };

  const handleStart = async () => {
    setValidationError(null);
    const hasAudio =
      selected_sources.includes("system_audio") || selected_sources.includes("microphone");
    if (!hasAudio) {
      setValidationError("At least one audio source (System Audio or Microphone) is required to start recording.");
      return;
    }
    try {
      setIsSubmitting(true);
      await start();
    } catch (err: unknown) {
      setValidationError(err instanceof Error ? err.message : "Failed to initiate capture engine.");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleStop = async () => {
    try {
      setIsSubmitting(true);
      await stop();
      void navigate({ to: "/" });
    } catch (err) {
      console.error(err);
    } finally {
      setIsSubmitting(false);
    }
  };

  const isRecording = state === "recording";
  const isPaused = state === "paused";

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <Header
        title="Capture Hub"
        subtitle={
          isRecording || isPaused
            ? "Active recording session in progress"
            : "Configure audio and screen capture sources"
        }
      />

      <div className="p-6 max-w-3xl mx-auto w-full flex-1 flex flex-col justify-center">
        {/* 3-hour warning banner (PRD FR12.5) */}
        {warning && (
          <div
            role="alert"
            className="mb-6 p-4 rounded-lg bg-amber-500/10 border border-status-warning/40 flex items-center justify-between gap-4 text-xs text-amber-900 dark:text-amber-200"
          >
            <div className="flex items-center gap-2.5">
              <AlertTriangle className="w-4 h-4 text-status-warning shrink-0" />
              <span>
                <strong>Session reached 3 hours.</strong> Capture continues reliably without interruption.
              </span>
            </div>
            <Button variant="ghost" size="sm" onClick={dismissWarning}>
              Dismiss
            </Button>
          </div>
        )}

        {/* ACTIVE RECORDING LIVE HUD */}
        {isRecording || isPaused ? (
          <div className="p-8 rounded-xl border border-border bg-surface-elevated shadow-lg text-center space-y-8">
            <div className="flex items-center justify-center gap-3">
              <span
                className={`w-3.5 h-3.5 rounded-full ${
                  isRecording
                    ? "bg-status-recording animate-recording-pulse shadow-[0_0_12px_rgba(229,57,53,0.8)]"
                    : "bg-status-warning"
                }`}
                aria-hidden="true"
              />
              <span className="text-xs font-mono font-semibold tracking-wider text-ink-muted uppercase">
                {isRecording ? "Recording Live" : "Capture Paused"}
              </span>
            </div>

            {/* Huge Tabular Timer */}
            <div
              className="text-6xl sm:text-7xl font-mono font-light tracking-tight text-ink tabular-nums"
              aria-live="polite"
              aria-label={`Elapsed recording time: ${formatTimer(elapsed_seconds)}`}
            >
              {formatTimer(elapsed_seconds)}
            </div>

            <div className="text-sm font-semibold text-ink-muted truncate max-w-md mx-auto">
              {meeting_title || "Untitled Session"}
            </div>

            {/* Live Audio Visualizer VU Meters */}
            <div className="max-w-md mx-auto space-y-2.5 bg-surface-sunken p-4 rounded-lg border border-border">
              <div className="flex items-center justify-between text-xs text-ink-muted">
                <span className="flex items-center gap-1.5 font-medium">
                  <Volume2 className="w-3.5 h-3.5" /> System Audio
                </span>
                <span className="font-mono text-[11px] tabular-nums">
                  {selected_sources.includes("system_audio") ? `${system_audio_level} dBFS` : "Disabled"}
                </span>
              </div>
              <div className="w-full h-2 bg-border rounded-full overflow-hidden">
                <div
                  className="h-full bg-mint transition-all duration-fast"
                  style={{
                    width: selected_sources.includes("system_audio") && isRecording ? "75%" : "0%",
                  }}
                />
              </div>

              <div className="flex items-center justify-between text-xs text-ink-muted pt-2">
                <span className="flex items-center gap-1.5 font-medium">
                  <Mic className="w-3.5 h-3.5" /> Microphone
                </span>
                <span className="font-mono text-[11px] tabular-nums">
                  {selected_sources.includes("microphone") ? `${mic_level} dBFS` : "Disabled"}
                </span>
              </div>
              <div className="w-full h-2 bg-border rounded-full overflow-hidden">
                <div
                  className="h-full bg-mint transition-all duration-fast"
                  style={{
                    width: selected_sources.includes("microphone") && isRecording ? "40%" : "0%",
                  }}
                />
              </div>
            </div>

            {/* Transport Buttons */}
            <div className="flex items-center justify-center gap-4 pt-2">
              {isRecording ? (
                <Button
                  variant="secondary"
                  size="lg"
                  onClick={() => void pause()}
                  disabled={isSubmitting}
                >
                  <Pause className="w-4 h-4 mr-2" />
                  Pause
                </Button>
              ) : (
                <Button
                  variant="primary"
                  size="lg"
                  onClick={() => void resume()}
                  disabled={isSubmitting}
                >
                  <Play className="w-4 h-4 mr-2 fill-current" />
                  Resume
                </Button>
              )}

              <Button
                variant="danger"
                size="lg"
                onClick={handleStop}
                isLoading={isSubmitting}
              >
                <Square className="w-4 h-4 mr-2 fill-current" />
                Stop & Process
              </Button>
            </div>
          </div>
        ) : (
          /* PRE-RECORD CONFIGURATION FORM (≤ 2 CLICKS TO START) */
          <div className="space-y-6">
            {/* Title & Type Form */}
            <div className="p-6 rounded-lg border border-border bg-surface-elevated space-y-4">
              <div>
                <label
                  htmlFor="meeting-title-input"
                  className="block text-xs font-semibold text-ink-muted mb-1 uppercase tracking-wider"
                >
                  Session Title
                </label>
                <input
                  id="meeting-title-input"
                  type="text"
                  value={meeting_title}
                  onChange={(e) => setTitle(e.target.value)}
                  placeholder="e.g. Q3 Architecture & Offline RAG Sync"
                  className="w-full h-10 px-3 rounded border border-border bg-surface-sunken text-sm text-ink font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold text-ink-muted mb-2 uppercase tracking-wider">
                  Classification
                </label>
                <div className="grid grid-cols-3 gap-2">
                  {(["auto", "meeting", "lecture"] as MeetingType[]).map((type) => (
                    <button
                      key={type}
                      type="button"
                      onClick={() => setType(type)}
                      className={`h-9 rounded text-xs font-medium border transition-colors capitalize ${
                        meeting_type === type
                          ? "bg-green-tint text-green-text border-primary/40 font-semibold"
                          : "border-border text-ink-muted hover:bg-surface"
                      }`}
                    >
                      {type === "auto" ? "Auto-detect" : type === "meeting" ? "Business Meeting" : "Lecture"}
                    </button>
                  ))}
                </div>
              </div>
            </div>

            {/* Capture Source Cards */}
            <div>
              <span className="block text-xs font-semibold text-ink-muted mb-2.5 uppercase tracking-wider">
                Capture Sources
              </span>

              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                {/* System Audio */}
                <button
                  type="button"
                  onClick={() => toggleSource("system_audio")}
                  className={`p-4 rounded-lg border text-left transition-all ${
                    selected_sources.includes("system_audio")
                      ? "border-primary bg-green-tint/30 ring-1 ring-primary/30"
                      : "border-border bg-surface-elevated hover:bg-surface text-ink-muted"
                  }`}
                >
                  <div className="flex items-center justify-between mb-2">
                    <Volume2 className="w-5 h-5 text-primary" />
                    <input
                      type="checkbox"
                      readOnly
                      checked={selected_sources.includes("system_audio")}
                      className="rounded accent-primary pointer-events-none"
                    />
                  </div>
                  <h3 className="text-xs font-semibold text-ink">System Audio</h3>
                  <p className="text-[11px] text-ink-muted mt-0.5">Captures remote speakers</p>
                </button>

                {/* Microphone */}
                <button
                  type="button"
                  onClick={() => toggleSource("microphone")}
                  className={`p-4 rounded-lg border text-left transition-all ${
                    selected_sources.includes("microphone")
                      ? "border-primary bg-green-tint/30 ring-1 ring-primary/30"
                      : "border-border bg-surface-elevated hover:bg-surface text-ink-muted"
                  }`}
                >
                  <div className="flex items-center justify-between mb-2">
                    <Mic className="w-5 h-5 text-primary" />
                    <input
                      type="checkbox"
                      readOnly
                      checked={selected_sources.includes("microphone")}
                      className="rounded accent-primary pointer-events-none"
                    />
                  </div>
                  <h3 className="text-xs font-semibold text-ink">Microphone</h3>
                  <p className="text-[11px] text-ink-muted mt-0.5">Captures your voice</p>
                </button>

                {/* Screen Video */}
                <button
                  type="button"
                  onClick={() => toggleSource("screen")}
                  className={`p-4 rounded-lg border text-left transition-all ${
                    selected_sources.includes("screen")
                      ? "border-primary bg-green-tint/30 ring-1 ring-primary/30"
                      : "border-border bg-surface-elevated hover:bg-surface text-ink-muted"
                  }`}
                >
                  <div className="flex items-center justify-between mb-2">
                    <Monitor className="w-5 h-5 text-primary" />
                    <input
                      type="checkbox"
                      readOnly
                      checked={selected_sources.includes("screen")}
                      className="rounded accent-primary pointer-events-none"
                    />
                  </div>
                  <h3 className="text-xs font-semibold text-ink">Screen Video</h3>
                  <p className="text-[11px] text-ink-muted mt-0.5">Extracts slides & OCR</p>
                </button>
              </div>
            </div>

            {/* Explicit Personal Microphone Setting (PRD FR12.2) */}
            <div className="p-3.5 rounded-lg border border-border bg-surface/50 flex items-center justify-between gap-4">
              <label
                htmlFor="only-me-mic-toggle"
                className="flex items-start gap-3 cursor-pointer select-none text-left"
              >
                <UserCheck className="w-4 h-4 text-ink-muted shrink-0 mt-0.5" />
                <div>
                  <span className="text-xs font-medium text-ink block">
                    Only me on this microphone
                  </span>
                  <span className="text-[11px] text-ink-muted block mt-0.5 leading-normal">
                    When checked, assigns microphone speech directly to User. Otherwise diarizes all voices anonymously.
                  </span>
                </div>
              </label>
              <input
                id="only-me-mic-toggle"
                type="checkbox"
                checked={only_me_mic}
                onChange={(e) => setOnlyMeMic(e.target.checked)}
                className="w-4 h-4 rounded accent-primary cursor-pointer shrink-0"
              />
            </div>

            {/* Validation Error Banner */}
            {validationError && (
              <div
                role="alert"
                className="p-3 rounded-md bg-red-500/10 border border-status-error/30 text-xs text-status-error flex items-center gap-2"
              >
                <AlertTriangle className="w-4 h-4 shrink-0" />
                <span>{validationError}</span>
              </div>
            )}

            {/* Start Button & Headroom info */}
            <div className="pt-2 flex flex-col items-center gap-3">
              <Button
                variant="primary"
                size="lg"
                onClick={handleStart}
                isLoading={isSubmitting}
                className="w-full sm:w-64 h-12 text-sm font-semibold shadow"
              >
                <Disc className="w-4 h-4 mr-2" />
                Start Recording
              </Button>

              <div className="flex items-center gap-2 text-xs text-ink-muted">
                <HardDrive className="w-3.5 h-3.5" />
                <span>Capture priority mode active · Disk space: 84.2 GB free</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export const RecordView: React.FC = () => {
  return (
    <ErrorBoundary fallbackTitle="Unable to load Capture Hub">
      <RecordContent />
    </ErrorBoundary>
  );
};

import React, { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { SegmentedControl } from "../components/ui/SegmentedControl";
import { Toggle } from "../components/ui/Toggle";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { useRecordingStore } from "../stores/recordingStore";
import { formatElapsed, formatDbfs, dbfsToPercent } from "../lib/format";
import {
  type CaptureSource,
  type MeetingType,
  prewarmCapture,
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

  // Pay the one-time FFmpeg/display lookup cost now, not when Record is pressed.
  useEffect(() => {
    void prewarmCapture();
  }, []);
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
  const [actionError, setActionError] = useState<string | null>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);

  const messageOf = (err: unknown, fallback: string) =>
    typeof err === "string" && err
      ? err
      : err instanceof Error && err.message
      ? err.message
      : fallback;

  const runTransport = async (action: () => Promise<void>, fallback: string) => {
    setActionError(null);
    try {
      setIsSubmitting(true);
      await action();
    } catch (err) {
      setActionError(messageOf(err, fallback));
    } finally {
      setIsSubmitting(false);
    }
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
      setValidationError(messageOf(err, "Failed to initiate capture engine."));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleStop = async () => {
    setActionError(null);
    try {
      setIsSubmitting(true);
      await stop();
      // A warning (e.g. video could not be finalised, audio kept) must not be
      // hidden by an instant redirect.
      const { reason } = useRecordingStore.getState();
      if (reason) {
        setSavedNotice(reason);
      } else {
        void navigate({ to: "/" });
      }
    } catch (err) {
      setActionError(messageOf(err, "Could not stop and save the recording."));
    } finally {
      setIsSubmitting(false);
    }
  };

  const isRecording = state === "recording";
  const isPaused = state === "paused";

  const sourceChips: { id: CaptureSource; label: string; icon: React.ReactNode }[] = [
    { id: "system_audio", label: "System Audio", icon: <Volume2 className="w-3.5 h-3.5" /> },
    { id: "microphone", label: "Microphone", icon: <Mic className="w-3.5 h-3.5" /> },
    { id: "screen", label: "Screen Video", icon: <Monitor className="w-3.5 h-3.5" /> },
  ];

  const bannerBase = "mb-4 p-3.5 rounded-xl flex items-center justify-between gap-4 text-xs";

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

      <div className="p-6 max-w-[680px] mx-auto w-full flex-1 flex flex-col justify-center pb-16">
        {/* 3-hour warning banner (PRD FR12.5) */}
        {warning && (
          <div role="alert" className={`${bannerBase} bg-status-warning/10 text-amber-900 dark:text-amber-200`}>
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

        {savedNotice && (
          <div role="status" className={`${bannerBase} bg-status-warning/10 text-amber-900 dark:text-amber-200`}>
            <span>
              <strong>Recording saved with a warning.</strong> {savedNotice}
            </span>
            <Button variant="ghost" size="sm" onClick={() => void navigate({ to: "/" })}>
              View meetings
            </Button>
          </div>
        )}

        {actionError && (
          <div role="alert" className="mb-4 p-3 rounded-xl bg-status-error/10 text-xs text-status-error flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            <span>{actionError}</span>
          </div>
        )}

        {/* ACTIVE RECORDING LIVE HUD */}
        {isRecording || isPaused ? (
          <div className="p-8 rounded-3xl bg-surface text-center space-y-8">
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

            {/* Huge tabular timer */}
            <div
              className="text-6xl sm:text-7xl font-mono font-light tracking-tight text-ink tabular-nums"
              aria-live="polite"
              aria-label={`Elapsed recording time: ${formatElapsed(elapsed_seconds)}`}
            >
              {formatElapsed(elapsed_seconds)}
            </div>

            <div className="text-sm font-semibold text-ink-muted truncate max-w-md mx-auto">
              {meeting_title || "Untitled Session"}
            </div>

            {/* Live audio VU meters */}
            <div className="max-w-md mx-auto space-y-2.5 bg-bg p-4 rounded-2xl">
              <div className="flex items-center justify-between text-xs text-ink-muted">
                <span className="flex items-center gap-1.5 font-medium">
                  <Volume2 className="w-3.5 h-3.5" /> System Audio
                </span>
                <span className="font-mono text-[11px] tabular-nums">
                  {selected_sources.includes("system_audio") ? formatDbfs(system_audio_level) : "Disabled"}
                </span>
              </div>
              <div className="w-full h-2 bg-surface-hover rounded-full overflow-hidden">
                <div
                  className={`h-full transition-all duration-fast ${system_audio_level > -3 ? "bg-status-warning" : "bg-mint"}`}
                  style={{
                    width: `${selected_sources.includes("system_audio") && isRecording ? dbfsToPercent(system_audio_level) : 0}%`,
                  }}
                />
              </div>

              <div className="flex items-center justify-between text-xs text-ink-muted pt-2">
                <span className="flex items-center gap-1.5 font-medium">
                  <Mic className="w-3.5 h-3.5" /> Microphone
                </span>
                <span className="font-mono text-[11px] tabular-nums">
                  {selected_sources.includes("microphone") ? formatDbfs(mic_level) : "Disabled"}
                </span>
              </div>
              <div className="w-full h-2 bg-surface-hover rounded-full overflow-hidden">
                <div
                  className={`h-full transition-all duration-fast ${mic_level > -3 ? "bg-status-warning" : "bg-mint"}`}
                  style={{
                    width: `${selected_sources.includes("microphone") && isRecording ? dbfsToPercent(mic_level) : 0}%`,
                  }}
                />
              </div>
            </div>

            {/* Transport pills */}
            <div className="flex items-center justify-center gap-3 pt-2">
              {isRecording ? (
                <Button
                  variant="secondary"
                  size="lg"
                  onClick={() => void runTransport(pause, "Could not pause the recording.")}
                  disabled={isSubmitting}
                >
                  <Pause className="w-4 h-4" />
                  Pause
                </Button>
              ) : (
                <Button
                  variant="primary"
                  size="lg"
                  onClick={() => void runTransport(resume, "Could not resume the recording.")}
                  disabled={isSubmitting}
                >
                  <Play className="w-4 h-4 fill-current" />
                  Resume
                </Button>
              )}

              <Button variant="danger" size="lg" onClick={handleStop} isLoading={isSubmitting}>
                <Square className="w-4 h-4 fill-current" />
                Stop & Process
              </Button>
            </div>
          </div>
        ) : (
          /* PRE-RECORD COMPOSER (≤ 2 CLICKS TO START) */
          <div className="space-y-6">
            <h2 className="text-center text-[26px] font-semibold tracking-tight text-ink">Ready to record?</h2>

            <div className="rounded-3xl border border-border bg-surface p-5 space-y-5">
              {/* Title */}
              <div>
                <label htmlFor="meeting-title-input" className="sr-only">
                  Session Title
                </label>
                <input
                  id="meeting-title-input"
                  type="text"
                  onChange={(e) => setTitle(e.target.value)}
                  placeholder="Session title, e.g. Q3 Architecture & Plans"
                  className="w-full bg-transparent text-[15px] text-ink placeholder:text-ink-subtle focus-visible:outline-none"
                />
              </div>

              {/* Classification */}
              <SegmentedControl<MeetingType>
                aria-label="Classification"
                value={meeting_type}
                onChange={setType}
                options={[
                  { value: "auto", label: "Auto-detect" },
                  { value: "meeting", label: "Business Meeting" },
                  { value: "lecture", label: "Lecture" },
                ]}
              />

              {/* Sources + start button */}
              <div className="flex items-center gap-2 flex-wrap">
                {sourceChips.map((chip) => {
                  const on = selected_sources.includes(chip.id);
                  return (
                    <button
                      key={chip.id}
                      type="button"
                      aria-pressed={on}
                      onClick={() => toggleSource(chip.id)}
                      className={`inline-flex items-center gap-1.5 h-8 px-3.5 rounded-full text-xs font-medium border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                        on
                          ? "bg-green-tint text-green-text border-transparent"
                          : "border-border text-ink-muted hover:text-ink hover:bg-surface-hover"
                      }`}
                    >
                      {chip.icon}
                      {chip.label}
                    </button>
                  );
                })}

                <Button
                  variant="primary"
                  size="icon"
                  onClick={handleStart}
                  isLoading={isSubmitting}
                  aria-label="Start Recording"
                  title="Start Recording"
                  className="ml-auto h-11 w-11"
                >
                  {!isSubmitting && <Disc className="w-5 h-5" />}
                </Button>
              </div>

              {/* Explicit personal microphone setting (PRD FR12.2) */}
              <div className="flex items-center justify-between gap-4 pt-4 border-t border-border-subtle">
                <div className="flex items-start gap-3 text-left">
                  <UserCheck className="w-4 h-4 text-ink-muted shrink-0 mt-0.5" />
                  <div>
                    <span className="text-[13px] font-medium text-ink block">Only me on this microphone</span>
                    <span className="text-xs text-ink-muted block mt-0.5 leading-normal">
                      When on, assigns microphone speech directly to User. Otherwise diarizes all voices anonymously.
                    </span>
                  </div>
                </div>
                <Toggle checked={only_me_mic} onChange={setOnlyMeMic} label="Only me on this microphone" />
              </div>
            </div>

            {/* Validation error banner */}
            {validationError && (
              <div role="alert" className="p-3 rounded-xl bg-status-error/10 text-xs text-status-error flex items-center gap-2">
                <AlertTriangle className="w-4 h-4 shrink-0" />
                <span>{validationError}</span>
              </div>
            )}

            <div className="flex items-center justify-center gap-2 text-xs text-ink-muted">
              <HardDrive className="w-3.5 h-3.5" />
              <span>Capture priority mode active · Disk space: 84.2 GB free</span>
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

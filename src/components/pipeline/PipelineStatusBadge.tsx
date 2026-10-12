import React, { useState } from "react";
import { type PipelineStatusDTO, retryPipelineStep } from "../../lib/tauri";
import {
  CheckCircle2,
  Clock,
  AlertTriangle,
  RotateCw,
  XCircle,
  HelpCircle,
  ChevronDown,
  Activity,
} from "lucide-react";
import { Button } from "../ui/Button";

export interface PipelineStatusBadgeProps {
  statusList: PipelineStatusDTO[];
  onRetryStep?: (jobId: string) => void;
}

export const PipelineStatusBadge: React.FC<PipelineStatusBadgeProps> = ({
  statusList,
  onRetryStep,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [retryingJobId, setRetryingJobId] = useState<string | null>(null);

  const hasError = statusList.some((s) => s.state === "error");
  const isRunning = statusList.some((s) => s.state === "running");
  const hasBlocked = statusList.some((s) => s.state === "blocked");
  const hasPending = statusList.some((s) => s.state === "pending");
  // Skipped optional steps (no slides, no diarization model) still count as finished.
  const allDone =
    statusList.length > 0 &&
    statusList.every((s) => s.state === "done" || s.state === "skipped");

  const runningStep = statusList.find((s) => s.state === "running");
  const errorStep = statusList.find((s) => s.state === "error");
  const blockedStep = statusList.find((s) => s.state === "blocked");

  const handleRetry = async (jobId: string) => {
    setRetryingJobId(jobId);
    try {
      await retryPipelineStep(jobId);
      onRetryStep?.(jobId);
    } finally {
      setRetryingJobId(null);
    }
  };

  const getStatusIcon = (state: string) => {
    switch (state) {
      case "done":
        return <CheckCircle2 className="w-3.5 h-3.5 text-status-success" />;
      case "running":
        return <RotateCw className="w-3.5 h-3.5 text-primary animate-spin" />;
      case "error":
        return <XCircle className="w-3.5 h-3.5 text-status-error" />;
      case "blocked":
        return <AlertTriangle className="w-3.5 h-3.5 text-status-warning" />;
      case "skipped":
        return <span className="text-[10px] font-mono text-ink-subtle">SKIPPED</span>;
      default:
        return <Clock className="w-3.5 h-3.5 text-ink-subtle" />;
    }
  };

  const formatStepName = (kind: string) => {
    return kind
      .replace(/_/g, " ")
      .replace(/\b\w/g, (c) => c.toUpperCase());
  };

  return (
    <div className="relative inline-block text-left">
      {/* Pill Trigger */}
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold whitespace-nowrap shrink-0 transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
          hasError
            ? "bg-status-error/10 text-status-error"
            : hasBlocked
            ? "bg-status-warning/10 text-status-warning"
            : isRunning
            ? "bg-green-tint text-green-text"
            : allDone
            ? "bg-green-tint text-green-text"
            : "bg-surface-hover text-ink-muted"
        }`}
        aria-expanded={isOpen}
        aria-haspopup="dialog"
      >
        {hasError ? (
          <>
            <XCircle className="w-3.5 h-3.5 text-status-error" />
            <span>Pipeline Error ({errorStep ? formatStepName(errorStep.kind) : ""})</span>
          </>
        ) : hasBlocked ? (
          <>
            <AlertTriangle className="w-3.5 h-3.5 text-status-warning" />
            <span>Action needed ({blockedStep ? formatStepName(blockedStep.kind) : ""})</span>
          </>
        ) : isRunning ? (
          <>
            <RotateCw className="w-3.5 h-3.5 text-primary animate-spin" />
            <span>
              {runningStep ? formatStepName(runningStep.kind) : "Processing"} ({runningStep?.progress || 0}%)
            </span>
          </>
        ) : hasPending && !allDone ? (
          <>
            <Clock className="w-3.5 h-3.5 text-ink-subtle" />
            <span>Queued for processing</span>
          </>
        ) : (
          <>
            <CheckCircle2 className="w-3.5 h-3.5 text-status-success" />
            <span>All Pipeline Steps Complete</span>
          </>
        )}
        <ChevronDown className="w-3 h-3 opacity-70" />
      </button>

      {/* Dropdown Menu / Details Modal */}
      {isOpen && (
        <div
          role="dialog"
          aria-label="Granular pipeline status details"
          className="absolute right-0 top-10 z-50 w-80 rounded-2xl border border-border bg-surface-elevated p-4 shadow-xl space-y-3"
        >
          <div className="flex items-center justify-between pb-2">
            <div className="flex items-center gap-1.5 text-xs font-semibold text-ink">
              <Activity className="w-3.5 h-3.5 text-primary" />
              <span>Pipeline Execution Graph</span>
            </div>
            <button
              type="button"
              onClick={() => setIsOpen(false)}
              className="text-ink-muted hover:text-ink text-xs p-1"
            >
              ✕
            </button>
          </div>

          <div className="space-y-2">
            {statusList.map((step) => {
              const isStepRetrying = retryingJobId === step.job_id;

              return (
                <div
                  key={step.job_id}
                  className="p-3 rounded-xl bg-bg space-y-1.5"
                >
                  <div className="flex items-center justify-between text-xs">
                    <div className="flex items-center gap-2">
                      {getStatusIcon(step.state)}
                      <span className="font-semibold text-ink">
                        {formatStepName(step.kind)}
                      </span>
                    </div>

                    <span
                      className={`font-mono text-[11px] capitalize ${
                        step.state === "done"
                          ? "text-status-success"
                          : step.state === "error"
                          ? "text-status-error"
                          : "text-ink-muted"
                      }`}
                    >
                      {step.state}
                      {step.state === "running" && ` (${step.progress}%)`}
                    </span>
                  </div>

                  {/* Reason, plus a Retry button when the user can act on it */}
                  {(step.state === "error" || step.state === "blocked") && (
                    <div
                      className={`mt-1 flex items-center justify-between gap-2 pt-1 border-t border-border/40 text-[11px] ${
                        step.state === "error" ? "text-status-error" : "text-status-warning"
                      }`}
                    >
                      <span className="whitespace-normal break-words">
                        {step.reason ||
                          (step.state === "error"
                            ? "Step failed during execution"
                            : "This step is waiting for you")}
                      </span>
                      <button
                        type="button"
                        onClick={() => handleRetry(step.job_id)}
                        disabled={isStepRetrying}
                        className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full bg-surface-hover hover:bg-border text-ink text-[11px] font-semibold shrink-0 transition-colors"
                      >
                        <RotateCw className={`w-2.5 h-2.5 ${isStepRetrying ? "animate-spin" : ""}`} />
                        <span>Retry</span>
                      </button>
                    </div>
                  )}
                  {step.state === "done" && (step.reason || step.kind !== "encoding") && (
                    <div className="mt-1 flex items-start justify-between gap-2 pt-1 border-t border-border/40 text-[11px] text-ink-muted">
                      <span className="whitespace-normal break-words">{step.reason}</span>
                      {step.kind !== "encoding" && (
                        <button
                          type="button"
                          onClick={() => handleRetry(step.job_id)}
                          disabled={isStepRetrying}
                          className="shrink-0 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-ink-muted hover:bg-surface-hover hover:text-ink transition-colors"
                          title="Run this step again; the steps that depend on it are redone too"
                        >
                          <RotateCw className="w-2.5 h-2.5" />
                          <span>Re-run</span>
                        </button>
                      )}
                    </div>
                  )}
                  {step.state === "skipped" && step.reason && (
                    <div className="mt-1 pt-1 border-t border-border/40 text-[11px] text-ink-muted whitespace-normal break-words">
                      {step.reason}
                    </div>
                  )}

                  {/* Progress bar for running step */}
                  {step.state === "running" && (
                    <div className="w-full h-1 bg-surface-hover rounded-full overflow-hidden">
                      <div
                        className="h-full bg-primary transition-all duration-300"
                        style={{ width: `${step.progress}%` }}
                      />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};

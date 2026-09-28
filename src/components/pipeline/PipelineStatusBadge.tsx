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
  const allDone = statusList.length > 0 && statusList.every((s) => s.state === "done");

  const runningStep = statusList.find((s) => s.state === "running");
  const errorStep = statusList.find((s) => s.state === "error");

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
        className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold border transition-all duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
          hasError
            ? "border-status-error/40 bg-red-50 text-status-error dark:bg-red-950/30"
            : isRunning
            ? "border-primary/40 bg-green-tint text-green-text"
            : allDone
            ? "border-green-text/20 bg-green-tint/50 text-green-text"
            : "border-border bg-surface text-ink-muted"
        }`}
        aria-expanded={isOpen}
        aria-haspopup="dialog"
      >
        {hasError ? (
          <>
            <XCircle className="w-3.5 h-3.5 text-status-error" />
            <span>Pipeline Error ({errorStep ? formatStepName(errorStep.kind) : ""})</span>
          </>
        ) : isRunning ? (
          <>
            <RotateCw className="w-3.5 h-3.5 text-primary animate-spin" />
            <span>
              {runningStep ? formatStepName(runningStep.kind) : "Processing"} ({runningStep?.progress || 0}%)
            </span>
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
          className="absolute right-0 top-9 z-50 w-80 rounded-lg border border-border bg-surface-elevated p-3.5 shadow-xl space-y-3"
        >
          <div className="flex items-center justify-between pb-2 border-b border-border">
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
                  className="p-2.5 rounded border border-border/80 bg-surface/30 space-y-1.5"
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

                  {/* Failure reason & Individual Step Retry Button */}
                  {step.state === "error" && (
                    <div className="mt-1 flex items-center justify-between gap-2 pt-1 border-t border-border/40 text-[11px] text-status-error">
                      <span className="truncate">{step.reason || "Step failed during execution"}</span>
                      <button
                        type="button"
                        onClick={() => handleRetry(step.job_id)}
                        disabled={isStepRetrying}
                        className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-surface hover:bg-surface-sunken text-ink text-[11px] font-semibold border border-border shrink-0 transition-colors"
                      >
                        <RotateCw className={`w-2.5 h-2.5 ${isStepRetrying ? "animate-spin" : ""}`} />
                        <span>Retry</span>
                      </button>
                    </div>
                  )}

                  {/* Progress bar for running step */}
                  {step.state === "running" && (
                    <div className="w-full h-1 bg-surface-sunken rounded-full overflow-hidden">
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

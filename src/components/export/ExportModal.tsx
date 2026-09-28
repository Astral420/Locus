import React, { useState } from "react";
import {
  type MeetingDTO,
  type SummaryRevisionDTO,
  type ActionItemDTO,
  type TranscriptSegmentDTO,
  type SlideDTO,
  exportMeetingAsMarkdown,
  exportMeetingAsJson,
  exportMeetingAsTxt,
} from "../../lib/tauri";
import { Download, Copy, FileText, Check, X, Printer, Code } from "lucide-react";
import { Button } from "../ui/Button";

export interface ExportModalProps {
  isOpen: boolean;
  onClose: () => void;
  meeting: MeetingDTO;
  summary: SummaryRevisionDTO;
  actionItems: ActionItemDTO[];
  segments: TranscriptSegmentDTO[];
  slides: SlideDTO[];
}

export const ExportModal: React.FC<ExportModalProps> = ({
  isOpen,
  onClose,
  meeting,
  summary,
  actionItems,
  segments,
  slides,
}) => {
  const [copied, setCopied] = useState(false);

  if (!isOpen) return null;

  const downloadFile = (filename: string, content: string, mimeType: string) => {
    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  const handleCopySummary = async () => {
    const md = exportMeetingAsMarkdown(meeting, summary, actionItems, segments);
    try {
      await navigator.clipboard.writeText(md);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback
    }
  };

  const handleExportMarkdown = () => {
    const md = exportMeetingAsMarkdown(meeting, summary, actionItems, segments);
    const filename = `${meeting.title.toLowerCase().replace(/[^a-z0-9]/g, "_")}.md`;
    downloadFile(filename, md, "text/markdown");
    onClose();
  };

  const handleExportJson = () => {
    const json = exportMeetingAsJson(meeting, summary, actionItems, segments, slides);
    const filename = `${meeting.title.toLowerCase().replace(/[^a-z0-9]/g, "_")}.json`;
    downloadFile(filename, json, "application/json");
    onClose();
  };

  const handleExportTxt = () => {
    const txt = exportMeetingAsTxt(meeting, segments);
    const filename = `${meeting.title.toLowerCase().replace(/[^a-z0-9]/g, "_")}_transcript.txt`;
    downloadFile(filename, txt, "text/plain");
    onClose();
  };

  const handleExportPdf = () => {
    window.print();
    onClose();
  };

  return (
    <div
      role="dialog"
      aria-labelledby="export-modal-title"
      aria-modal="true"
      className="fixed inset-0 z-50 bg-black/60 backdrop-blur-xs flex items-center justify-center p-4 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-lg border border-border bg-surface-elevated p-6 shadow-2xl space-y-5"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between pb-3 border-b border-border">
          <div>
            <h2 id="export-modal-title" className="text-base font-semibold text-ink">
              Export Meeting Content
            </h2>
            <p className="text-xs text-ink-muted mt-0.5">
              Export synchronized meeting intelligence to durable local formats
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-1 rounded text-ink-muted hover:text-ink hover:bg-surface focus-visible:outline-none"
            aria-label="Close export modal"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Quick Clipboard Copy Section */}
        <div className="p-3.5 rounded-lg border border-border bg-surface/40 flex items-center justify-between gap-3">
          <div className="space-y-0.5">
            <span className="text-xs font-semibold text-ink flex items-center gap-1.5">
              <Copy className="w-3.5 h-3.5 text-primary" />
              <span>One-Click Clipboard Copy</span>
            </span>
            <p className="text-[11px] text-ink-muted">
              Copy executive summary, key decisions, and action items formatted for Slack or Docs
            </p>
          </div>

          <Button variant="secondary" size="sm" onClick={handleCopySummary}>
            {copied ? (
              <>
                <Check className="w-3.5 h-3.5 mr-1 text-primary" />
                Copied!
              </>
            ) : (
              <>
                <Copy className="w-3.5 h-3.5 mr-1" />
                Copy (C)
              </>
            )}
          </Button>
        </div>

        {/* Export Formats Grid */}
        <div className="space-y-2">
          <span className="text-xs font-semibold text-ink-muted uppercase tracking-wider text-[11px]">
            File Downloads
          </span>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
            {/* Markdown Export */}
            <button
              type="button"
              onClick={handleExportMarkdown}
              className="p-3 rounded-lg border border-border bg-surface-elevated hover:bg-surface hover:border-primary/40 transition-all text-left flex items-start gap-2.5 group"
            >
              <FileText className="w-4 h-4 text-primary shrink-0 mt-0.5 group-hover:scale-110 transition-transform" />
              <div>
                <span className="text-xs font-semibold text-ink block">Markdown (.md)</span>
                <span className="text-[11px] text-ink-muted">Full summary + transcript</span>
              </div>
            </button>

            {/* JSON Export */}
            <button
              type="button"
              onClick={handleExportJson}
              className="p-3 rounded-lg border border-border bg-surface-elevated hover:bg-surface hover:border-primary/40 transition-all text-left flex items-start gap-2.5 group"
            >
              <Code className="w-4 h-4 text-primary shrink-0 mt-0.5 group-hover:scale-110 transition-transform" />
              <div>
                <span className="text-xs font-semibold text-ink block">JSON (.json)</span>
                <span className="text-[11px] text-ink-muted">Structured session schema</span>
              </div>
            </button>

            {/* PDF Print / Export */}
            <button
              type="button"
              onClick={handleExportPdf}
              className="p-3 rounded-lg border border-border bg-surface-elevated hover:bg-surface hover:border-primary/40 transition-all text-left flex items-start gap-2.5 group"
            >
              <Printer className="w-4 h-4 text-primary shrink-0 mt-0.5 group-hover:scale-110 transition-transform" />
              <div>
                <span className="text-xs font-semibold text-ink block">PDF Document</span>
                <span className="text-[11px] text-ink-muted">Clean archival print</span>
              </div>
            </button>

            {/* TXT Transcript Export */}
            <button
              type="button"
              onClick={handleExportTxt}
              className="p-3 rounded-lg border border-border bg-surface-elevated hover:bg-surface hover:border-primary/40 transition-all text-left flex items-start gap-2.5 group"
            >
              <Download className="w-4 h-4 text-primary shrink-0 mt-0.5 group-hover:scale-110 transition-transform" />
              <div>
                <span className="text-xs font-semibold text-ink block">Plain Text (.txt)</span>
                <span className="text-[11px] text-ink-muted">Dialogue & timestamps</span>
              </div>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

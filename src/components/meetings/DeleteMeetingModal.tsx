import React from "react";
import { type MeetingDTO, deleteMeeting } from "../../lib/tauri";
import { AlertTriangle, Trash2, X } from "lucide-react";
import { Button } from "../ui/Button";

export interface DeleteMeetingModalProps {
  isOpen: boolean;
  onClose: () => void;
  meeting: MeetingDTO;
  onDeleted: (meetingId: string) => void;
}

export const DeleteMeetingModal: React.FC<DeleteMeetingModalProps> = ({
  isOpen,
  onClose,
  meeting,
  onDeleted,
}) => {
  const [isDeleting, setIsDeleting] = React.useState(false);

  if (!isOpen) return null;

  const handleDelete = async () => {
    setIsDeleting(true);
    try {
      await deleteMeeting(meeting.id);
      onDeleted(meeting.id);
      onClose();
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-labelledby="delete-meeting-title"
      aria-modal="true"
      className="fixed inset-0 z-50 bg-black/60 backdrop-blur-xs flex items-center justify-center p-4 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-2xl border border-border bg-surface-elevated p-6 shadow-2xl space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start gap-3">
          <div className="w-10 h-10 rounded-full bg-red-100 dark:bg-red-950/40 text-status-error flex items-center justify-center shrink-0">
            <Trash2 className="w-5 h-5" />
          </div>

          <div className="space-y-1">
            <h2 id="delete-meeting-title" className="text-sm font-semibold text-ink">
              Delete Meeting Record
            </h2>
            <p className="text-xs text-ink-muted leading-relaxed">
              Are you sure you want to permanently delete{" "}
              <span className="font-semibold text-ink">"{meeting.title}"</span>?
            </p>
          </div>
        </div>

        <div className="p-3 rounded-xl bg-status-error/10 text-xs text-red-900 dark:text-red-200">
          <p>
            This action immediately purges all captured audio, screen video segments, diarized transcripts, extracted slides, and summary revisions. Independent linked documents will be preserved.
          </p>
        </div>

        <div className="flex items-center justify-end gap-2 pt-2">
          <Button variant="secondary" size="sm" onClick={onClose} disabled={isDeleting}>
            Cancel
          </Button>
          <Button
            variant="danger"
            size="sm"
            onClick={handleDelete}
            disabled={isDeleting}
          >
            {isDeleting ? "Deleting…" : "Delete Meeting"}
          </Button>
        </div>
      </div>
    </div>
  );
};

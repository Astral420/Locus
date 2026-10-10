import React from "react";
import { Trash2 } from "lucide-react";
import { type MeetingDTO, deleteMeeting } from "../../lib/tauri";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";

export interface DeleteMeetingModalProps {
  isOpen: boolean;
  onClose: () => void;
  meeting: MeetingDTO;
  onDeleted: (meetingId: string) => void;
}

/** Confirmation dialog for deleting a meeting. Same anatomy as the Knowledge conversation delete dialog. */
export const DeleteMeetingModal: React.FC<DeleteMeetingModalProps> = ({ isOpen, onClose, meeting, onDeleted }) => {
  const [isDeleting, setIsDeleting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const close = () => {
    if (isDeleting) return;
    setError(null);
    onClose();
  };

  const handleDelete = async () => {
    setIsDeleting(true);
    setError(null);
    try {
      await deleteMeeting(meeting.id);
      onDeleted(meeting.id);
      onClose();
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : typeof reason === "string" ? reason : "The meeting could not be deleted."
      );
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={close}
      title="Delete meeting?"
      description={`“${meeting.title}” and all of its audio, video, transcripts, slides and summaries will be permanently deleted. Your linked documents are not affected.`}
    >
      {error && (
        <p role="alert" className="mb-3 rounded-xl bg-status-error/10 px-3 py-2 text-xs text-status-error">
          {error}
        </p>
      )}
      <div className="flex items-center justify-end gap-2">
        <Button variant="secondary" size="sm" onClick={close} disabled={isDeleting}>
          Cancel
        </Button>
        <Button variant="danger" size="sm" onClick={() => void handleDelete()} isLoading={isDeleting}>
          <Trash2 className="w-3.5 h-3.5" />
          Delete
        </Button>
      </div>
    </Modal>
  );
};

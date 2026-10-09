import React from "react";
import { Modal } from "./Modal";
import { Button } from "./Button";
import { AlertCircle, Clock, ShieldAlert } from "lucide-react";

export interface RecoveryModalProps {
  isOpen: boolean;
  onRecover: () => void;
  onDiscard: () => void;
  onClose: () => void;
  sessionDate?: string;
  durationFormatted?: string;
}

export const RecoveryModal: React.FC<RecoveryModalProps> = ({
  isOpen,
  onRecover,
  onDiscard,
  onClose,
  sessionDate = "Sep 26, 2026 at 13:42",
  durationFormatted = "34m 12s",
}) => {
  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Interrupted Recording Detected"
      description="Locus detected an unfinalized session from a previous unexpected shutdown."
    >
      <div className="space-y-4 text-sm text-ink">
        <div className="p-3.5 bg-amber-500/10 rounded-xl flex items-start gap-3 text-amber-800 dark:text-amber-200">
          <ShieldAlert className="w-5 h-5 shrink-0 mt-0.5 text-status-warning" />
          <p className="text-xs leading-relaxed">
            Durable media manifests preserved the captured segments. Crash recovery guarantees target lost tail ≤ 5 seconds.
          </p>
        </div>

        <div className="bg-surface-sunken p-3 rounded-xl grid grid-cols-2 gap-3 text-xs">
          <div>
            <span className="text-ink-muted block mb-0.5">Recorded At</span>
            <span className="font-medium text-ink flex items-center gap-1.5">
              <Clock className="w-3.5 h-3.5 text-ink-muted" />
              {sessionDate}
            </span>
          </div>
          <div>
            <span className="text-ink-muted block mb-0.5">Recoverable Duration</span>
            <span className="font-mono font-medium text-ink">{durationFormatted}</span>
          </div>
        </div>

        <div className="flex items-center justify-end gap-3 pt-3">
          <Button variant="secondary" onClick={onDiscard}>
            Discard Recording
          </Button>
          <Button variant="primary" onClick={onRecover}>
            Recover and Process
          </Button>
        </div>
      </div>
    </Modal>
  );
};

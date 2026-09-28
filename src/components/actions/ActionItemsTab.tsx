import React from "react";
import { type ActionItemDTO, toggleActionItem, getSpeakerPalette } from "../../lib/tauri";
import { useUiStore } from "../../stores/uiStore";
import { CheckSquare, Calendar, User, CheckCircle2, Circle } from "lucide-react";
import { Badge } from "../ui/Badge";

export interface ActionItemsTabProps {
  summaryRevisionId: string;
  actionItems: ActionItemDTO[];
  onToggleAction?: (id: string, completed: boolean) => void;
}

export const ActionItemsTab: React.FC<ActionItemsTabProps> = ({
  summaryRevisionId,
  actionItems,
  onToggleAction,
}) => {
  const theme = useUiStore((state) => state.theme);
  const isDark = theme === "dark";

  // Filter items scoped strictly to this summary revision
  const revisionItems = actionItems.filter(
    (item) => item.summary_revision_id === summaryRevisionId
  );

  const completedCount = revisionItems.filter((i) => i.completed).length;
  const pendingCount = revisionItems.length - completedCount;

  const handleToggle = async (item: ActionItemDTO) => {
    const nextCompleted = !item.completed;
    onToggleAction?.(item.id, nextCompleted);
    try {
      await toggleActionItem(item.id, nextCompleted);
    } catch {
      // Revert if failed
      onToggleAction?.(item.id, item.completed);
    }
  };

  return (
    <div className="space-y-4 max-w-2xl" role="region" aria-label="Revision Action Items">
      {/* Header Info */}
      <div className="flex items-center justify-between p-3 rounded-lg border border-border bg-surface/30 text-xs">
        <div className="flex items-center gap-2">
          <CheckSquare className="w-4 h-4 text-primary" />
          <span className="font-semibold text-ink">Action Items</span>
          <span className="text-ink-muted">· Revision {summaryRevisionId}</span>
        </div>

        <div className="flex items-center gap-2 text-ink-muted font-mono text-[11px]">
          <span className="px-2 py-0.5 rounded bg-surface border border-border">
            {pendingCount} Pending
          </span>
          <span className="px-2 py-0.5 rounded bg-surface border border-border">
            {completedCount} Completed
          </span>
        </div>
      </div>

      {/* Action Items List */}
      {revisionItems.length === 0 ? (
        <div className="py-12 text-center text-xs text-ink-muted">
          No action items assigned in this summary revision.
        </div>
      ) : (
        <div className="space-y-2.5" role="list">
          {revisionItems.map((item) => {
            const palette = item.assignee
              ? getSpeakerPalette(item.assignee, isDark)
              : null;

            return (
              <div
                key={item.id}
                role="listitem"
                onClick={() => handleToggle(item)}
                className={`p-3.5 rounded-lg border transition-all duration-fast cursor-pointer flex items-start gap-3 select-none ${
                  item.completed
                    ? "bg-surface/30 border-border/60 opacity-80"
                    : "bg-surface-elevated border-border hover:bg-surface/50 shadow-xs"
                }`}
              >
                {/* Checkbox */}
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={item.completed}
                  aria-label={`Mark task as ${item.completed ? "pending" : "completed"}: ${item.text}`}
                  className="mt-0.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded text-primary"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleToggle(item);
                  }}
                >
                  {item.completed ? (
                    <CheckCircle2 className="w-4.5 h-4.5 fill-primary text-white" />
                  ) : (
                    <Circle className="w-4.5 h-4.5 text-ink-muted hover:text-primary transition-colors" />
                  )}
                </button>

                {/* Content */}
                <div className="flex-1 min-w-0 space-y-1.5">
                  <p
                    className={`text-sm leading-relaxed transition-all ${
                      item.completed
                        ? "line-through text-ink-muted"
                        : "text-ink font-medium"
                    }`}
                  >
                    {item.text}
                  </p>

                  <div className="flex flex-wrap items-center gap-2.5 text-xs">
                    {/* Assignee Badge with Diarization Hue */}
                    {item.assignee ? (
                      <span
                        className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-semibold border"
                        style={
                          palette
                            ? {
                                color: palette.text,
                                backgroundColor: palette.bg,
                                borderColor: palette.border,
                              }
                            : undefined
                        }
                      >
                        <User className="w-2.5 h-2.5" />
                        <span>{item.assignee}</span>
                      </span>
                    ) : (
                      <span className="text-[11px] text-ink-subtle italic">Unassigned</span>
                    )}

                    {/* Deadline Pill */}
                    {item.deadline ? (
                      <span className="inline-flex items-center gap-1 font-mono text-[11px] text-ink-muted bg-surface px-2 py-0.5 rounded border border-border">
                        <Calendar className="w-2.5 h-2.5" />
                        <span>Due: {item.deadline}</span>
                      </span>
                    ) : (
                      <span className="text-[11px] text-ink-subtle">No deadline</span>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

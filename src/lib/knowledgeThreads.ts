import type { QueryClient } from "@tanstack/react-query";
import { createKnowledgeThread, type KnowledgeScope, type KnowledgeThreadDTO } from "./tauri";
import { useKnowledgeStore } from "../stores/knowledgeStore";

export const KNOWLEDGE_SCOPE_LABELS: Record<KnowledgeScope, string> = {
  this_meeting: "This meeting",
  all_meetings: "All meetings",
  documents_only: "Documents only",
  everything: "Everything",
};

/**
 * Open a conversation with the given fixed scope. An existing empty conversation with the same scope (and meeting)
 * is reused instead of creating another identical one, so repeated "+" clicks or scope toggles never pile up empties.
 */
export async function openKnowledgeThread(
  queryClient: QueryClient,
  threads: KnowledgeThreadDTO[],
  scope: KnowledgeScope,
  meetingId: string | null | undefined
): Promise<KnowledgeThreadDTO> {
  const reusable = threads.find(
    (t) => t.message_count === 0 && t.scope === scope && (t.meeting_id ?? null) === (meetingId ?? null)
  );
  const thread = reusable ?? (await createKnowledgeThread(scope, meetingId ?? null));
  useKnowledgeStore.getState().setSelectedThreadId(thread.id);
  if (!reusable) await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
  return thread;
}

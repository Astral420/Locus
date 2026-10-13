import type { QueryClient } from "@tanstack/react-query";
import {
  createKnowledgeThread,
  deleteKnowledgeThread,
  listKnowledgeThreads,
  type KnowledgeScope,
  type KnowledgeThreadDTO,
} from "./tauri";
import { useKnowledgeStore } from "../stores/knowledgeStore";

export const KNOWLEDGE_SCOPE_LABELS: Record<KnowledgeScope, string> = {
  this_meeting: "This meeting",
  all_meetings: "All meetings",
  documents_only: "Documents only",
  everything: "Everything",
};

/** A conversation counts (and is listed in the sidebar) only once something has actually been asked. */
export const visibleThreads = (threads: KnowledgeThreadDTO[]): KnowledgeThreadDTO[] =>
  threads.filter((thread) => thread.message_count > 0);

/**
 * Start a new, empty conversation with the given fixed scope. This only switches the page into draft mode:
 * no thread is created and nothing is added to the sidebar until the first message is sent.
 */
export function startKnowledgeDraft(scope: KnowledgeScope, meetingId?: string | null): void {
  useKnowledgeStore.getState().startDraft(scope, meetingId ?? null);
}

/** Creating the thread is deferred to the first send; callers use this right before sending. */
export async function createThreadForFirstMessage(
  queryClient: QueryClient,
  scope: KnowledgeScope,
  meetingId: string | null | undefined
): Promise<KnowledgeThreadDTO> {
  const thread = await createKnowledgeThread(scope, meetingId ?? null);
  // Refresh before selecting so the page can find the thread it just created.
  await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
  return thread;
}

/** If the first send failed before anything was stored, remove the empty thread so it never lingers. */
export async function discardIfEmpty(queryClient: QueryClient, threadId: string): Promise<void> {
  try {
    const current = await listKnowledgeThreads();
    const thread = current.find((candidate) => candidate.id === threadId);
    if (thread && thread.message_count === 0) await deleteKnowledgeThread(threadId);
  } catch {
    // best effort: an empty thread is hidden from the sidebar either way
  } finally {
    await queryClient.invalidateQueries({ queryKey: ["knowledgeThreads"] });
  }
}

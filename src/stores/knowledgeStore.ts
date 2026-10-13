import { create } from "zustand";
import type { KnowledgeScope } from "../lib/tauri";

/**
 * A conversation that exists only in the UI. Nothing is created in the backend
 * (or listed in the sidebar) until the first message is actually sent.
 */
export interface KnowledgeDraft {
  scope: KnowledgeScope;
  meetingId: string | null;
}

interface KnowledgeStoreState {
  /** Currently open Knowledge conversation; null means "the most recent one". Shared by the sidebar list and the chat page. */
  selectedThreadId: string | null;
  /** When set, the page shows a blank new conversation with this scope instead of any saved thread. */
  draft: KnowledgeDraft | null;
  /** Opening a saved conversation leaves draft mode; clearing the selection does not touch the draft. */
  setSelectedThreadId: (id: string | null) => void;
  startDraft: (scope: KnowledgeScope, meetingId?: string | null) => void;
}

export const useKnowledgeStore = create<KnowledgeStoreState>((set) => ({
  selectedThreadId: null,
  draft: null,
  setSelectedThreadId: (id) => set(id ? { selectedThreadId: id, draft: null } : { selectedThreadId: null }),
  startDraft: (scope, meetingId = null) => set({ selectedThreadId: null, draft: { scope, meetingId } }),
}));

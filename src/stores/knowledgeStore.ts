import { create } from "zustand";

interface KnowledgeStoreState {
  /** Currently open Knowledge conversation; null means "the most recent one". Shared by the sidebar list and the chat page. */
  selectedThreadId: string | null;
  setSelectedThreadId: (id: string | null) => void;
}

export const useKnowledgeStore = create<KnowledgeStoreState>((set) => ({
  selectedThreadId: null,
  setSelectedThreadId: (id) => set({ selectedThreadId: id }),
}));

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args), convertFileSrc: (p: string) => p }));

import * as tauri from "../lib/tauri";

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  invokeMock.mockReset();
});

describe("deleteKnowledgeThread (browser preview)", () => {
  it("removes the conversation and its messages from the mock data", async () => {
    const before = await tauri.listKnowledgeThreads();
    const target = before[0];
    expect((await tauri.listKnowledgeMessages(target.id)).length).toBeGreaterThan(0);

    await tauri.deleteKnowledgeThread(target.id);

    const after = await tauri.listKnowledgeThreads();
    expect(after.map((t) => t.id)).not.toContain(target.id);
    expect(after).toHaveLength(before.length - 1);
    expect(await tauri.listKnowledgeMessages(target.id)).toHaveLength(0);
  });
});

describe("deleteKnowledgeThread (inside Tauri)", () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  it("calls the backend command with the thread id", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauri.deleteKnowledgeThread("th-42");
    expect(invokeMock).toHaveBeenCalledWith("delete_knowledge_thread", { threadId: "th-42" });
  });

  it("surfaces backend errors instead of silently falling back to mock data", async () => {
    invokeMock.mockRejectedValueOnce("Conversation not found");
    await expect(tauri.deleteKnowledgeThread("missing")).rejects.toBe("Conversation not found");
    expect(invokeMock).toHaveBeenCalledTimes(1); // no retry / fallback path
  });
});

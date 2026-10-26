import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  convertFileSrc: (p: string) => p,
}));

// Settings is observed through the IPC layer: everything else stays real, but the
// persisted selection starts empty and the selection call is a spy.
vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    getSelectedProvider: vi.fn(async () => null),
    selectProvider: vi.fn(actual.selectProvider),
  };
});

import * as tauri from "../lib/tauri";
import { renderApp } from "./renderApp";

const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });
const NO_MODEL = "No local language model is selected. Download one in Models, then press Set active.";

const setTauri = (inside: boolean) => {
  if (inside) (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  else delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
};

afterEach(() => {
  cleanup();
  setTauri(false);
  invokeMock.mockReset();
});

describe("Knowledge chat and summaries inside Tauri", () => {
  beforeEach(() => setTauri(true));

  it("rejects a Knowledge answer with the backend message instead of a canned reply", async () => {
    invokeMock.mockRejectedValue(NO_MODEL);

    await expect(tauri.sendKnowledgeMessage("th-1", "What did we decide?")).rejects.toThrow(NO_MODEL);
    expect(invokeMock).toHaveBeenCalledWith("send_knowledge_message", { threadId: "th-1", content: "What did we decide?" });
    // One attempt: a rejection is never retried through the mock fallback.
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("rejects a summary request instead of inventing one", async () => {
    invokeMock.mockRejectedValue(NO_MODEL);

    await expect(tauri.triggerSummary("m-01")).rejects.toThrow(NO_MODEL);
    expect(invokeMock).toHaveBeenCalledWith("trigger_summary", { meetingId: "m-01", regenerate: false });
  });
});

describe("Settings provider selection", () => {
  it("persists the explicitly selected local provider when Select is pressed", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Llama.cpp" }));

    // Nothing is selected on a fresh install (ADR 0003): the button offers Select.
    const select = await screen.findByRole("button", { name: "Select" });
    fireEvent.click(select);

    await waitFor(() => expect(tauri.selectProvider).toHaveBeenCalledTimes(1));
    const [provider, model, destination, acceptDisclosure] = vi.mocked(tauri.selectProvider).mock.calls[0];
    expect(provider).toBe("llama_server");
    expect(model).toBe("llama-3.2-3b"); // the active generation model, not a remote one
    expect(destination).toBe("http://127.0.0.1 (local, offline)");
    expect(acceptDisclosure).toBe(true);

    expect(await screen.findByText("Selected (Active)")).toBeTruthy();
    expect(await screen.findByText(/Llama\.cpp is the selected provider/)).toBeTruthy();
  });
});

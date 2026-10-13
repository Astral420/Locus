import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import { useKnowledgeStore } from "../stores/knowledgeStore";
import * as tauri from "../lib/tauri";

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listKnowledgeThreads: vi.fn(actual.listKnowledgeThreads),
    createKnowledgeThread: vi.fn(actual.createKnowledgeThread),
    sendKnowledgeMessage: vi.fn(actual.sendKnowledgeMessage),
    deleteKnowledgeThread: vi.fn(actual.deleteKnowledgeThread),
  };
});
const actualTauri = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");

const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });
const thread = (over: Partial<tauri.KnowledgeThreadDTO> & { id: string }): tauri.KnowledgeThreadDTO => ({
  title: "New Knowledge inquiry",
  scope: "all_meetings",
  meeting_id: null,
  updated_at: "2026-10-09T00:00:00Z",
  message_count: 0,
  ...over,
});

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ sidebarCollapsed: false });
  useKnowledgeStore.setState({ selectedThreadId: null, draft: null });
  for (const fn of [tauri.createKnowledgeThread, tauri.sendKnowledgeMessage, tauri.deleteKnowledgeThread]) {
    vi.mocked(fn).mockClear(); // call history is shared across tests
  }
  vi.mocked(tauri.createKnowledgeThread).mockImplementation(actualTauri.createKnowledgeThread);
  vi.mocked(tauri.sendKnowledgeMessage).mockImplementation(actualTauri.sendKnowledgeMessage);
  vi.mocked(tauri.deleteKnowledgeThread).mockImplementation(actualTauri.deleteKnowledgeThread);
});
afterEach(() => {
  cleanup();
  vi.mocked(tauri.listKnowledgeThreads).mockImplementation(actualTauri.listKnowledgeThreads);
});

describe("Round 3: conversation history lives in the sidebar", () => {
  it("shows a Conversations section under the navigation on every page", async () => {
    renderApp("/settings");
    const aside = await screen.findByLabelText("Primary navigation");
    const section = within(aside).getByRole("region", { name: "Conversations" });
    expect(within(section).getByRole("button", { name: "New conversation" })).toBeTruthy();
    expect(await within(section).findAllByRole("listitem")).not.toHaveLength(0);
  });

  it("scrolls inside the sidebar instead of stacking: many threads stay in a bounded, scrollable list", async () => {
    const many = Array.from({ length: 40 }, (_, i) => thread({ id: `th-${i}`, message_count: 1 }));
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue(many);
    renderApp("/");
    const list = await screen.findByRole("list", { name: "Conversation history" });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(40));
    expect(list.className).toContain("overflow-y-auto");
    expect(list.className).toContain("min-h-0");
    expect(list.closest("section")!.className).toContain("min-h-0"); // flex child allowed to shrink so the list scrolls
    // The primary nav above it never shrinks away.
    expect(screen.getByLabelText("Primary navigation").querySelector("nav")!.className).toContain("shrink-0");
  });

  it("opens a conversation on the Knowledge page with its fixed scope", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([
      thread({ id: "a", title: "Alpha", scope: "all_meetings", message_count: 2 }),
      thread({ id: "b", title: "Beta", scope: "documents_only", message_count: 1 }),
    ]);
    renderApp("/settings");
    const aside = await screen.findByLabelText("Primary navigation");
    fireEvent.click(await within(aside).findByRole("button", { name: /Beta/ }));

    expect(await h1("Knowledge Base")).toBeTruthy();
    expect(useKnowledgeStore.getState().selectedThreadId).toBe("b");
    await waitFor(() => expect((screen.getByLabelText("Scope") as HTMLSelectElement).value).toBe("documents_only"));
    const row = within(aside).getByRole("button", { name: /Beta/ });
    expect(row.getAttribute("aria-current")).toBe("true");
    expect(within(aside).getByRole("button", { name: /Alpha/ }).getAttribute("aria-current")).toBeNull();
    expect(within(aside).getByText("Documents only · 1 message")).toBeTruthy();
  });

  it("the Knowledge page no longer carries its own conversation list", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    const sources = await screen.findByLabelText("Knowledge sources");
    expect(within(sources).queryByText(/conversations/i)).toBeNull();
    expect(within(sources).getByText(/Documents \(/)).toBeTruthy();
    expect(within(sources).getByLabelText("Semantic search")).toBeTruthy();
  });

  it("'+' only opens a blank Knowledge Base conversation: nothing is created and nothing is added to the sidebar", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([thread({ id: "busy", title: "Busy one", message_count: 3 })]);
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));

    expect(await h1("Knowledge Base")).toBeTruthy();
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(useKnowledgeStore.getState().draft).toEqual({ scope: "all_meetings", meetingId: null });
    expect(useKnowledgeStore.getState().selectedThreadId).toBeNull();
    // still just the one real conversation, and it is not highlighted: the blank draft is what is open
    expect(within(aside).getAllByRole("listitem")).toHaveLength(1);
    expect(within(aside).getByRole("button", { name: /Busy one/ }).getAttribute("aria-current")).toBeNull();
    expect(await screen.findByText("Start a Knowledge inquiry")).toBeTruthy();
  });

  it("pressing '+' repeatedly never piles anything up", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([thread({ id: "busy", message_count: 3 })]);
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    for (let i = 0; i < 3; i++) fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));
    await h1("Knowledge Base");
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(within(aside).getAllByRole("listitem")).toHaveLength(1);
  });

  it("changing scope creates nothing: it starts a blank conversation with that scope", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([
      thread({ id: "t1", scope: "all_meetings", message_count: 1 }),
      thread({ id: "docs-empty", scope: "documents_only", message_count: 0 }),
    ]);
    renderApp("/knowledge");
    await h1("Knowledge Base");
    const aside = screen.getByLabelText("Primary navigation");
    await waitFor(() => expect(within(aside).getAllByRole("listitem")).toHaveLength(1)); // the empty one is not listed
    const scope = (await screen.findByLabelText("Scope")) as HTMLSelectElement;
    fireEvent.change(scope, { target: { value: "documents_only" } });
    fireEvent.change(scope, { target: { value: "everything" } });

    await waitFor(() => expect(useKnowledgeStore.getState().draft?.scope).toBe("everything"));
    expect((screen.getByLabelText("Scope") as HTMLSelectElement).value).toBe("everything");
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(within(aside).getAllByRole("listitem")).toHaveLength(1);
  });

  it("conversations that never received a message are not listed in the sidebar", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([
      thread({ id: "e1", message_count: 0 }),
      thread({ id: "real", title: "Real chat", message_count: 2 }),
      thread({ id: "e2", message_count: 0, scope: "documents_only" }),
    ]);
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await waitFor(() => expect(within(aside).getAllByRole("listitem")).toHaveLength(1));
    expect(within(aside).getByRole("button", { name: /Real chat/ })).toBeTruthy();
    expect(within(aside).queryByText("New Knowledge inquiry")).toBeNull();
  });

  it("with only empty conversations the sidebar says there are none", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([thread({ id: "e1", message_count: 0 })]);
    renderApp("/");
    expect(await screen.findByText("No conversations yet")).toBeTruthy();
  });

  it("the first message is what creates the conversation, and only then does it appear in the sidebar", async () => {
    let created = false;
    let asked = false;
    const fresh = () => thread({ id: "fresh", title: "What did we decide?", message_count: asked ? 2 : 0 });
    vi.mocked(tauri.listKnowledgeThreads).mockImplementation(async () => [
      thread({ id: "busy", title: "Busy one", message_count: 3 }),
      ...(created ? [fresh()] : []),
    ]);
    vi.mocked(tauri.createKnowledgeThread).mockImplementation(async () => {
      created = true;
      return fresh();
    });
    vi.mocked(tauri.sendKnowledgeMessage).mockImplementation(async (threadId) => {
      asked = true;
      return {
        id: "a1", thread_id: threadId, ordinal: 1, role: "assistant", content: "We decided to ship.",
        state: "complete", provider_identity: "llama_server", model_identity: "m", created_at: "2026-10-12T00:00:00Z", citations: [],
      };
    });
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));
    await h1("Knowledge Base");

    // typing a question is not enough; nothing exists until it is sent
    fireEvent.change(await screen.findByLabelText("Knowledge question"), { target: { value: "What did we decide?" } });
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(within(aside).getAllByRole("listitem")).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(tauri.createKnowledgeThread).toHaveBeenCalledTimes(1));
    expect(tauri.createKnowledgeThread).toHaveBeenCalledWith("all_meetings", null);
    await waitFor(() => expect(tauri.sendKnowledgeMessage).toHaveBeenCalledWith("fresh", "What did we decide?"));
    await waitFor(() => expect(within(aside).getAllByRole("listitem")).toHaveLength(2));
    expect(useKnowledgeStore.getState().selectedThreadId).toBe("fresh");
    expect(useKnowledgeStore.getState().draft).toBeNull();
  });

  it("if the first question cannot be answered, the empty conversation is removed and the question is kept", async () => {
    let created = false;
    vi.mocked(tauri.listKnowledgeThreads).mockImplementation(async () => [
      thread({ id: "busy", message_count: 3 }),
      ...(created ? [thread({ id: "fresh", message_count: 0 })] : []),
    ]);
    vi.mocked(tauri.createKnowledgeThread).mockImplementation(async () => {
      created = true;
      return thread({ id: "fresh", message_count: 0 });
    });
    vi.mocked(tauri.sendKnowledgeMessage).mockRejectedValue(new Error("No provider selected"));
    vi.mocked(tauri.deleteKnowledgeThread).mockImplementation(async () => {
      created = false;
    });
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));
    await h1("Knowledge Base");
    fireEvent.change(await screen.findByLabelText("Knowledge question"), { target: { value: "hello?" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(tauri.deleteKnowledgeThread).toHaveBeenCalledWith("fresh"));
    expect(await screen.findByText(/No provider selected/)).toBeTruthy();
    expect((screen.getByLabelText("Knowledge question") as HTMLInputElement).value).toBe("hello?");
    expect(within(aside).getAllByRole("listitem")).toHaveLength(1);
    expect(useKnowledgeStore.getState().draft).not.toBeNull();
  });

  it("opening a meeting does not create a conversation; its chat creates one only when a question is sent", async () => {
    let created = false;
    vi.mocked(tauri.createKnowledgeThread).mockImplementation(async (scope, meetingId) => {
      created = true;
      return thread({ id: "mt", scope, meeting_id: meetingId ?? null, message_count: 0 });
    });
    vi.mocked(tauri.sendKnowledgeMessage).mockImplementation(async (threadId) => ({
      id: "a1", thread_id: threadId, ordinal: 1, role: "assistant", content: "ok", state: "complete",
      provider_identity: null, model_identity: null, created_at: "2026-10-12T00:00:00Z", citations: [],
    }));
    renderApp("/meeting/m-01");
    await h1(/Q3 Architecture/);
    fireEvent.click(await screen.findByRole("button", { name: /^chat$/i }));
    const input = await screen.findByPlaceholderText("Ask about this meeting…");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(created).toBe(false);

    fireEvent.change(input, { target: { value: "Who owns the sidecar work?" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    await waitFor(() => expect(tauri.createKnowledgeThread).toHaveBeenCalledWith("this_meeting", "m-01"));
    await waitFor(() => expect(tauri.sendKnowledgeMessage).toHaveBeenCalledWith("mt", "Who owns the sidecar work?"));
    expect(tauri.createKnowledgeThread).toHaveBeenCalledTimes(1);
  });

  it("gives the scope bar breathing room so a focus ring is never clipped at the top", async () => {
    renderApp("/knowledge");
    const scope = await screen.findByLabelText("Scope");
    const bar = scope.closest("div")!;
    expect(bar.className).toContain("pt-1.5");
    expect(bar.className).toContain("px-1");
  });
});

describe("Round 3: clicking anywhere clears pill highlights", () => {
  it("tracks pointer vs keyboard use on the document root", async () => {
    renderApp("/");
    await h1("Meetings");
    expect(document.documentElement.dataset.inputModality).toBe("pointer");

    fireEvent.keyDown(document.body, { key: "Tab" });
    expect(document.documentElement.dataset.inputModality).toBe("keyboard");
    fireEvent.keyDown(document.body, { key: "Shift" }); // a bare modifier does not change the mode
    expect(document.documentElement.dataset.inputModality).toBe("keyboard");

    fireEvent.mouseDown(document.body);
    expect(document.documentElement.dataset.inputModality).toBe("pointer");
  });

  it("blurs a focused pill when the user clicks on empty space, but not when clicking another control", async () => {
    renderApp("/");
    const select = (await screen.findByLabelText("Filter meetings by classification")) as HTMLSelectElement;
    select.focus();
    expect(document.activeElement).toBe(select);

    fireEvent.mouseDown(document.querySelector("header")!); // non-interactive space
    expect(document.activeElement).not.toBe(select);

    select.focus();
    fireEvent.mouseDown(screen.getByLabelText("Search meetings")); // another control: focus handling is left to the browser
    expect(document.activeElement).toBe(select);
  });

  it("removes the modality attribute when the app unmounts", async () => {
    const { unmount } = renderApp("/");
    await h1("Meetings");
    unmount();
    expect(document.documentElement.dataset.inputModality).toBeUndefined();
  });

  it("styles: the pointer-mode rule hides rings on pills and buttons, and keyboard mode is not affected", () => {
    const css = readFileSync(join(process.cwd(), "src", "styles.css"), "utf8");
    expect(css).toContain(':root[data-input-modality="pointer"]');
    const rule = css.slice(css.indexOf(':root[data-input-modality="pointer"]'));
    expect(rule.slice(0, rule.indexOf("}"))).toMatch(/outline:\s*none/);
    expect(css).not.toContain('data-input-modality="keyboard"');
  });
});

describe("Round 3: window chrome", () => {
  const conf = JSON.parse(readFileSync(join(process.cwd(), "src-tauri", "tauri.conf.json"), "utf8")); // vitest runs from the repo root
  const win = conf.app.windows[0];

  it("uses the overlay title bar with the traffic lights centred on the sidebar's top row", () => {
    expect(win.titleBarStyle).toBe("Overlay");
    expect(win.hiddenTitle).toBe(true);
    expect(win.trafficLightPosition).toEqual({ x: 20, y: 30 });
  });
});

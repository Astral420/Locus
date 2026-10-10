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
    deleteKnowledgeThread: vi.fn(actual.deleteKnowledgeThread),
    sendKnowledgeMessage: vi.fn(actual.sendKnowledgeMessage),
  };
});
const actualTauri = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");

const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });
const thread = (over: Partial<tauri.KnowledgeThreadDTO> & { id: string }): tauri.KnowledgeThreadDTO => ({
  title: `Thread ${over.id}`,
  scope: "all_meetings",
  meeting_id: null,
  updated_at: "2026-10-09T00:00:00Z",
  message_count: 2,
  ...over,
});

let threadsState: tauri.KnowledgeThreadDTO[] = [];
const useThreads = (list: tauri.KnowledgeThreadDTO[]) => {
  threadsState = list;
  vi.mocked(tauri.listKnowledgeThreads).mockImplementation(async () => threadsState);
  vi.mocked(tauri.deleteKnowledgeThread).mockImplementation(async (id) => {
    threadsState = threadsState.filter((t) => t.id !== id);
  });
};

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ sidebarCollapsed: false });
  useKnowledgeStore.setState({ selectedThreadId: null });
  vi.mocked(tauri.deleteKnowledgeThread).mockClear();
  vi.mocked(tauri.listKnowledgeThreads).mockClear();
});
afterEach(() => {
  cleanup();
  vi.mocked(tauri.listKnowledgeThreads).mockImplementation(actualTauri.listKnowledgeThreads);
  vi.mocked(tauri.deleteKnowledgeThread).mockImplementation(actualTauri.deleteKnowledgeThread);
  vi.mocked(tauri.sendKnowledgeMessage).mockImplementation(actualTauri.sendKnowledgeMessage);
});

const openList = async () => {
  const aside = await screen.findByLabelText("Primary navigation");
  const list = await within(aside).findByRole("list", { name: "Conversation history" });
  await waitFor(() => expect(within(list).queryAllByRole("listitem").length).toBeGreaterThan(0));
  return { aside, list };
};
const rowOf = (list: HTMLElement, title: string) =>
  within(list).getAllByRole("listitem").find((li) => li.textContent?.includes(title))!;

describe("Round 4: kebab menu to delete a conversation", () => {
  it("every conversation has a kebab button; the menu is closed until it is pressed", async () => {
    useThreads([thread({ id: "a" }), thread({ id: "b" })]);
    renderApp("/");
    const { list } = await openList();

    const kebabs = within(list).getAllByRole("button", { name: "Conversation options" });
    expect(kebabs).toHaveLength(2);
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.click(kebabs[0]);
    expect(kebabs[0].getAttribute("aria-expanded")).toBe("true");
    const menu = screen.getByRole("menu", { name: "Conversation options" });
    expect(within(menu).getByRole("menuitem", { name: "Delete conversation" })).toBeTruthy();
  });

  it("closes on Escape and on an outside press without deleting anything", async () => {
    useThreads([thread({ id: "a" })]);
    renderApp("/");
    const { list } = await openList();
    const kebab = within(list).getByRole("button", { name: "Conversation options" });

    fireEvent.click(kebab);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

    fireEvent.click(kebab);
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.mouseDown(document.body);
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    expect(tauri.deleteKnowledgeThread).not.toHaveBeenCalled();
  });

  it("asks for confirmation, and Cancel leaves the conversation alone", async () => {
    useThreads([thread({ id: "a", title: "Sidecar notes", message_count: 3 })]);
    renderApp("/");
    const { list } = await openList();

    fireEvent.click(within(rowOf(list, "Sidecar notes")).getByRole("button", { name: "Conversation options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete conversation" }));

    const dialog = await screen.findByRole("dialog", { name: "Delete conversation?" });
    expect(within(dialog).getByText(/Sidecar notes/)).toBeTruthy();
    expect(within(dialog).getByText(/3 messages/)).toBeTruthy();
    expect(within(dialog).getByText(/meetings and documents are not affected/i)).toBeTruthy();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete conversation?" })).toBeNull());
    expect(tauri.deleteKnowledgeThread).not.toHaveBeenCalled();
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  });

  it("Delete removes only that conversation and refreshes the list", async () => {
    useThreads([thread({ id: "a", title: "Keep me" }), thread({ id: "b", title: "Remove me" })]);
    renderApp("/");
    const { list } = await openList();

    fireEvent.click(within(rowOf(list, "Remove me")).getByRole("button", { name: "Conversation options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete conversation" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete conversation?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(tauri.deleteKnowledgeThread).toHaveBeenCalledWith("b"));
    await waitFor(() => expect(screen.queryByText("Remove me")).toBeNull());
    expect(screen.getByText("Keep me")).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete conversation?" })).toBeNull());
  });

  it("deleting the open conversation clears the selection; deleting another one does not", async () => {
    useThreads([thread({ id: "a", title: "Open one" }), thread({ id: "b", title: "Other one" })]);
    useKnowledgeStore.setState({ selectedThreadId: "a" });
    renderApp("/");
    const { list } = await openList();

    // delete the non-selected one first
    fireEvent.click(within(rowOf(list, "Other one")).getByRole("button", { name: "Conversation options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete conversation" }));
    fireEvent.click(within(await screen.findByRole("dialog", { name: "Delete conversation?" })).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByText("Other one")).toBeNull());
    expect(useKnowledgeStore.getState().selectedThreadId).toBe("a");

    // now the selected one
    fireEvent.click(within(rowOf(list, "Open one")).getByRole("button", { name: "Conversation options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete conversation" }));
    fireEvent.click(within(await screen.findByRole("dialog", { name: "Delete conversation?" })).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByText("Open one")).toBeNull());
    expect(useKnowledgeStore.getState().selectedThreadId).toBeNull();
    expect(await screen.findByText("No conversations yet")).toBeTruthy();
  });

  it("a failed delete keeps the dialog open, shows the error and keeps the conversation", async () => {
    useThreads([thread({ id: "a", title: "Stubborn" })]);
    vi.mocked(tauri.deleteKnowledgeThread).mockRejectedValueOnce(new Error("Conversation not found"));
    renderApp("/");
    const { list } = await openList();

    fireEvent.click(within(rowOf(list, "Stubborn")).getByRole("button", { name: "Conversation options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete conversation" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete conversation?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));

    expect((await within(dialog).findByRole("alert")).textContent).toBe("Conversation not found");
    expect(screen.getByRole("dialog", { name: "Delete conversation?" })).toBeTruthy();
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  });
});

describe("Round 4: delete is wired through to the backend", () => {
  const read = (...parts: string[]) => readFileSync(join(process.cwd(), ...parts), "utf8");

  it("the Rust service, Tauri command and handler registration all exist", () => {
    const service = read("src-tauri", "src", "knowledge.rs");
    expect(service).toContain("pub fn delete_thread(&self, thread_id: &str)");
    expect(service).toContain('"DELETE FROM chat_threads WHERE id=?1"');

    const command = read("src-tauri", "src", "commands", "knowledge.rs");
    expect(command).toMatch(/pub fn delete_knowledge_thread\(\s*state: State<'_, AppState>,\s*thread_id: String,\s*\)/);
    expect(command).toContain("state.knowledge.delete_thread(&thread_id)");

    expect(read("src-tauri", "src", "lib.rs")).toContain("commands::knowledge::delete_knowledge_thread,");
  });

  it("the schema cascades a thread delete to its messages, citations and dependencies", () => {
    const sql = read("src-tauri", "migrations", "0001_initial.sql");
    expect(sql).toMatch(/thread_id TEXT NOT NULL REFERENCES chat_threads\(id\) ON DELETE CASCADE/);
    expect(sql).toMatch(/message_id TEXT NOT NULL REFERENCES chat_messages\(id\) ON DELETE CASCADE/);
  });
});

describe("Round 4: conversation messages look like a chat, not full-width blocks", () => {
  it("renders user messages as compact right-aligned bubbles and assistant messages as left-aligned cards", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    const user = await waitFor(() => {
      const el = document.querySelector('[data-role="user"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });
    const assistant = document.querySelector('[data-role="assistant"]') as HTMLElement;
    expect(assistant).not.toBeNull();

    // alignment
    expect(user.className).toContain("justify-end");
    expect(assistant.className).toContain("justify-start");
    // bubbles size to their content and are capped, instead of spanning the whole conversation container
    const userBubble = user.firstElementChild as HTMLElement;
    const assistantBubble = assistant.firstElementChild as HTMLElement;
    expect(userBubble.className).toContain("w-fit");
    expect(userBubble.className).toContain("max-w-[75%]");
    expect(assistantBubble.className).toContain("w-fit");
    expect(assistantBubble.className).toContain("max-w-[88%]");
    // neutral user bubble (no full-width brand-green block)
    expect(userBubble.className).toContain("bg-surface-hover");
    expect(userBubble.className).not.toContain("bg-primary");
    // the whole thread sits in a bounded, centred column
    expect(user.parentElement!.className).toContain("max-w-3xl");
    expect(user.parentElement!.className).toContain("mx-auto");
  });

  it("shows a timestamp on each message", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    await waitFor(() => expect(document.querySelectorAll("[data-role]").length).toBeGreaterThan(0));
    for (const el of Array.from(document.querySelectorAll("[data-role]"))) {
      expect(el.textContent).toMatch(/\d{1,2}:\d{2}/);
    }
  });

  it("lists sources as chips under a collapsible 'Sources (N)' row", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    const toggle = await screen.findByRole("button", { name: /Sources \(\d+\)/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getAllByTitle("Citation location").length).toBeGreaterThan(0);

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryAllByTitle("Citation location")).toHaveLength(0);

    fireEvent.click(toggle);
    expect(screen.getAllByTitle("Citation location").length).toBeGreaterThan(0);
  });

  it("refreshes the sidebar conversation list after a message is sent (count and title come from the backend)", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    const input = await screen.findByLabelText("Knowledge question");
    await waitFor(() => expect(tauri.listKnowledgeThreads).toHaveBeenCalled());
    const before = vi.mocked(tauri.listKnowledgeThreads).mock.calls.length;

    fireEvent.change(input, { target: { value: "What did we decide?" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(vi.mocked(tauri.listKnowledgeThreads).mock.calls.length).toBeGreaterThan(before));
    expect(await screen.findByText("What did we decide?", { selector: "p" })).toBeTruthy();
  });
});

describe("Round 4: smaller changes", () => {
  it("General settings no longer lists Automatic telemetry", async () => {
    renderApp("/settings");
    await h1("Settings");
    expect(screen.getByText("Configured providers")).toBeTruthy();
    expect(screen.getByText("Updates")).toBeTruthy();
    expect(screen.queryByText(/automatic telemetry/i)).toBeNull();
    expect(screen.queryByText("Nothing is sent from this device.")).toBeNull();
  });

  it("the hidden sidebar keeps the same control order as the open one (downloads first, then the toggle)", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    const open = Array.from(aside.querySelectorAll("button")).slice(0, 2).map((b) => b.getAttribute("aria-label"));
    expect(open).toEqual(["Downloads", "Hide sidebar"]);

    fireEvent.click(within(aside).getByRole("button", { name: "Hide sidebar" }));
    const toolbar = await screen.findByRole("toolbar", { name: "Sidebar controls" });
    const hidden = Array.from(toolbar.querySelectorAll("button")).map((b) => b.getAttribute("aria-label"));
    expect(hidden).toEqual(["Downloads", "Show sidebar"]);
  });
});

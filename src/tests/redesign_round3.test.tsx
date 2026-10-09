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
  useKnowledgeStore.setState({ selectedThreadId: null });
  vi.mocked(tauri.createKnowledgeThread).mockClear(); // call history is shared across tests
  vi.mocked(tauri.createKnowledgeThread).mockImplementation(actualTauri.createKnowledgeThread);
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
    const many = Array.from({ length: 40 }, (_, i) => thread({ id: `th-${i}` }));
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

  it("'+' reuses an existing empty conversation with the same scope instead of stacking another", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([
      thread({ id: "busy", message_count: 3 }),
      thread({ id: "empty", message_count: 0, scope: "all_meetings" }),
    ]);
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));

    expect(await h1("Knowledge Base")).toBeTruthy();
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
    expect(useKnowledgeStore.getState().selectedThreadId).toBe("empty");
  });

  it("'+' creates a conversation when none is empty, then opens it", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([thread({ id: "busy", message_count: 3 })]);
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    await within(aside).findAllByRole("listitem");
    fireEvent.click(within(aside).getByRole("button", { name: "New conversation" }));

    await waitFor(() => expect(tauri.createKnowledgeThread).toHaveBeenCalledWith("all_meetings", null));
    expect(await h1("Knowledge Base")).toBeTruthy();
  });

  it("changing scope on the page reuses an empty conversation of that scope rather than creating another", async () => {
    vi.mocked(tauri.listKnowledgeThreads).mockResolvedValue([
      thread({ id: "t1", scope: "all_meetings", message_count: 1 }),
      thread({ id: "docs-empty", scope: "documents_only", message_count: 0 }),
    ]);
    renderApp("/knowledge");
    await h1("Knowledge Base");
    await within(screen.getByLabelText("Primary navigation")).findByRole("button", { name: /docs-empty|New Knowledge inquiry/ }).catch(() => undefined);
    await waitFor(() => expect(within(screen.getByLabelText("Primary navigation")).getAllByRole("listitem")).toHaveLength(2));
    const scope = (await screen.findByLabelText("Scope")) as HTMLSelectElement;
    fireEvent.change(scope, { target: { value: "documents_only" } });

    await waitFor(() => expect(useKnowledgeStore.getState().selectedThreadId).toBe("docs-empty"));
    expect(tauri.createKnowledgeThread).not.toHaveBeenCalled();
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

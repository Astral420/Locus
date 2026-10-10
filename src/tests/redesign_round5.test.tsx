import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, within, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import { useKnowledgeStore } from "../stores/knowledgeStore";
import * as tauri from "../lib/tauri";
import { DeleteMeetingModal } from "../components/meetings/DeleteMeetingModal";

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    deleteMeeting: vi.fn(actual.deleteMeeting),
    listKnowledgeThreads: vi.fn(actual.listKnowledgeThreads),
    deleteKnowledgeThread: vi.fn(actual.deleteKnowledgeThread),
  };
});
const actualTauri = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");

const meeting = actualTauri.MOCK_MEETINGS[0];
const src = (...parts: string[]) => readFileSync(join(process.cwd(), "src", ...parts), "utf8");
const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ sidebarCollapsed: false });
  useKnowledgeStore.setState({ selectedThreadId: null });
  vi.mocked(tauri.deleteMeeting).mockClear();
});
afterEach(() => {
  cleanup();
  vi.mocked(tauri.deleteMeeting).mockImplementation(actualTauri.deleteMeeting);
});

describe("Round 5: delete-meeting dialog matches the conversation delete dialog", () => {
  const setup = (over: Partial<React.ComponentProps<typeof DeleteMeetingModal>> = {}) => {
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(<DeleteMeetingModal isOpen meeting={meeting} onClose={onClose} onDeleted={onDeleted} {...over} />);
    return { onClose, onDeleted };
  };

  it("has the same anatomy: question title, one muted sentence, Cancel + Delete (no icon card, no red warning box)", () => {
    setup();
    const dialog = screen.getByRole("dialog", { name: "Delete meeting?" });
    expect(within(dialog).getByRole("heading", { name: "Delete meeting?" })).toBeTruthy();
    expect(within(dialog).getByText(new RegExp(meeting.title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")))).toBeTruthy();
    expect(within(dialog).getByText(/permanently deleted/)).toBeTruthy();
    expect(within(dialog).getByText(/linked documents are not affected/i)).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Delete" })).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Close dialog" })).toBeTruthy();
    // the old design's trash-can badge, red warning panel and bespoke title are gone
    expect(dialog.querySelector(".bg-red-100")).toBeNull();
    expect(dialog.querySelector(".bg-status-error\\/10")).toBeNull();
    expect(screen.queryByText("Delete Meeting Record")).toBeNull();
  });

  it("is built from the same shared Modal as the conversation dialog", () => {
    const meetingModal = src("components", "meetings", "DeleteMeetingModal.tsx");
    const sidebar = src("components", "layout", "Sidebar.tsx");
    expect(meetingModal).toContain('from "../ui/Modal"');
    expect(sidebar).toContain('from "../ui/Modal"');
    // same button language in both
    for (const code of [meetingModal, sidebar]) {
      expect(code).toContain('variant="secondary"');
      expect(code).toContain('variant="danger"');
      expect(code).toContain("<Trash2");
    }
    expect(meetingModal).toContain("rounded-xl bg-status-error/10 px-3 py-2 text-xs text-status-error");
    expect(sidebar).toContain("rounded-xl bg-status-error/10 px-3 py-2 text-xs text-status-error");
  });

  it("Cancel closes without deleting", () => {
    const { onClose, onDeleted } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(tauri.deleteMeeting).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("Delete removes the meeting, reports it and closes", async () => {
    vi.mocked(tauri.deleteMeeting).mockResolvedValueOnce(undefined); // don't mutate the shared mock library
    const { onClose, onDeleted } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(tauri.deleteMeeting).toHaveBeenCalledWith(meeting.id));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledWith(meeting.id));
    expect(onClose).toHaveBeenCalled();
  });

  it("a failed delete keeps the dialog open and shows the error instead of failing silently", async () => {
    vi.mocked(tauri.deleteMeeting).mockRejectedValueOnce(new Error("Disk is read-only"));
    const { onClose, onDeleted } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect((await screen.findByRole("alert")).textContent).toBe("Disk is read-only");
    expect(onDeleted).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Delete meeting?" })).toBeTruthy();
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("works from the Meetings library end to end", async () => {
    vi.mocked(tauri.deleteMeeting).mockResolvedValueOnce(undefined);
    renderApp("/");
    await screen.findByText(meeting.title);
    fireEvent.click(screen.getAllByTitle("Delete meeting")[0]);
    const dialog = await screen.findByRole("dialog", { name: "Delete meeting?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(tauri.deleteMeeting).toHaveBeenCalledWith(meeting.id);
  });
});

describe("Round 5: Meeting Detail top bar", () => {
  it("gives the title room from the window edge and wraps actions instead of squeezing the title", async () => {
    renderApp("/meeting/m-01");
    const title = await h1(new RegExp(meeting.title));
    const bar = title.closest("div.px-6")!;
    expect(bar.className).toContain("pt-4"); // was pt-1: the title sat against the top edge
    const row = bar.firstElementChild as HTMLElement;
    expect(row.className).toContain("flex-wrap");
    expect(row.className).not.toContain("flex-col");
    // title block may shrink and truncates; the action group never shrinks
    expect(title.parentElement!.className).toContain("min-w-0");
    expect(title.querySelector("span.truncate")).not.toBeNull();
    expect(row.lastElementChild!.className).toContain("shrink-0");
  });

  it("keeps the meta line and the pipeline pill on one line each", async () => {
    renderApp("/meeting/m-01");
    const title = await h1(new RegExp(meeting.title));
    const meta = title.nextElementSibling as HTMLElement;
    expect(meta.className).toContain("whitespace-nowrap");
    const pill = await screen.findByRole("button", { name: /all pipeline steps complete/i });
    expect(pill.className).toContain("whitespace-nowrap");
    expect(pill.className).toContain("shrink-0");
  });
});

describe("Round 5: Meeting Detail responds to window size", () => {
  it("stacks into one scrolling column below lg and only becomes a fixed two-pane layout from lg up", async () => {
    renderApp("/meeting/m-01");
    await h1(new RegExp(meeting.title));
    const grid = document.querySelector(".grid.lg\\:grid-cols-12") as HTMLElement;
    expect(grid).not.toBeNull();
    // narrow / tall windows: the page scrolls and rows size to their content (no stretched empty cards)
    expect(grid.className).toContain("grid-cols-1");
    expect(grid.className).toContain("overflow-y-auto");
    expect(grid.className).toContain("auto-rows-min");
    // wide windows: panes fill the height and scroll independently
    expect(grid.className).toContain("lg:overflow-hidden");
    expect(grid.className).toContain("lg:auto-rows-auto");

    const [left, right] = Array.from(grid.children) as HTMLElement[];
    expect(left.className).toContain("lg:overflow-y-auto");
    expect(left.className).not.toMatch(/(^|\s)overflow-y-auto/); // not forced to scroll internally when stacked
    expect(right.className).toContain("min-h-[34rem]"); // stacked tab panel never collapses to a sliver
    expect(right.className).toContain("lg:min-h-0");
  });

  it("hides tab icons only in the cramped two-pane width range so all five tabs fit", async () => {
    renderApp("/meeting/m-01");
    await h1(new RegExp(meeting.title));
    const tabs = ["Summary", "Transcript", "Action Items", "Slides", "Chat"].map((name) =>
      screen.getByRole("button", { name: new RegExp(`^${name}`) })
    );
    for (const tab of tabs) {
      const icon = tab.querySelector("svg") as SVGElement;
      expect(icon.getAttribute("class")).toContain("lg:max-xl:hidden");
    }
  });
});

describe("Round 5: Settings uses the width of a large window", () => {
  it("fills the available width instead of stopping at a fixed 720px column", () => {
    const settings = src("routes", "SettingsView.tsx");
    expect(settings).not.toContain("max-w-[720px]");
    expect(settings).not.toContain("max-w-[1120px]");
    expect(settings).toContain('<div className="flex-1 min-w-0 overflow-y-auto space-y-3">');
  });

  it("keeps text fields a readable width even though the cards are wide", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "OpenAI" }));
    for (const label of ["OpenAI endpoint", "OpenAI model", "API Key (Redacted)"]) {
      expect((await screen.findByLabelText(label)).className).toContain("max-w-[640px]");
    }
  });
});

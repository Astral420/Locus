import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor, act } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import { useRecordingStore } from "../stores/recordingStore";

const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({
    sidebarCollapsed: false,
    downloadStatus: { active: false, modelName: "", progress: 0, etaSeconds: 0 },
  });
  useRecordingStore.setState({ state: "idle", elapsed_seconds: 0 });
});
afterEach(() => cleanup());

describe("Redesign shell: sidebar", () => {
  it("renders the five nav destinations with shortcut hints and marks the current page", async () => {
    renderApp("/settings");
    const aside = await screen.findByLabelText("Primary navigation");
    const nav = within(aside);
    for (const name of ["Recording", "Meetings", "Knowledge Base", "Model Manager", "Settings"]) {
      expect(nav.getByRole("link", { name })).toBeTruthy();
    }
    expect(nav.getByRole("link", { name: "Settings" }).getAttribute("aria-current")).toBe("page");
    expect(nav.getByRole("link", { name: "Meetings" }).getAttribute("aria-current")).toBeNull();

    // Shortcut hints are decorative (aria-hidden) and show on Recording, Meetings and Settings only.
    const hints = aside.querySelectorAll('[aria-hidden="true"].nums-tabular');
    expect(hints.length).toBe(3);
  });

  it("has no recent-meetings list (nav-only sidebar)", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    expect(within(aside).queryByText(/^chats$/i)).toBeNull();
    expect(within(aside).queryByText(/recent meetings/i)).toBeNull();
    expect(within(aside).queryByText("Q3 Architecture & Offline RAG Sync")).toBeNull();
  });

  it("navigates when a nav item is clicked", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("link", { name: "Model Manager" }));
    expect(await h1("Model Manager")).toBeTruthy();
    fireEvent.click(within(aside).getByRole("link", { name: "Knowledge Base" }));
    expect(await h1("Knowledge Base")).toBeTruthy();
  });

  it("hides completely when collapsed (no icon rail), persists the choice, and can be reopened", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("button", { name: "Hide sidebar" }));

    expect(useUiStore.getState().sidebarCollapsed).toBe(true);
    expect(localStorage.getItem("locus_sidebar_collapsed")).toBe("true");
    // No rail: the navigation landmark and its links are gone; only the small reopen toolbar remains.
    expect(screen.queryByLabelText("Primary navigation")).toBeNull();
    expect(screen.queryByRole("link", { name: "Knowledge Base" })).toBeNull();
    const toolbar = screen.getByRole("toolbar", { name: "Sidebar controls" });
    expect(within(toolbar).getByRole("button", { name: "Downloads" })).toBeTruthy();

    fireEvent.click(within(toolbar).getByRole("button", { name: "Show sidebar" }));
    expect(useUiStore.getState().sidebarCollapsed).toBe(false);
    expect(await screen.findByLabelText("Primary navigation")).toBeTruthy();
    expect(screen.queryByRole("toolbar", { name: "Sidebar controls" })).toBeNull();
  });

  it("toggles with Ctrl/Cmd+B", async () => {
    renderApp("/");
    await screen.findByLabelText("Primary navigation");
    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    await waitFor(() => expect(screen.queryByLabelText("Primary navigation")).toBeNull());
    fireEvent.keyDown(window, { key: "b", metaKey: true });
    expect(await screen.findByLabelText("Primary navigation")).toBeTruthy();
  });

  it("keeps page titles clear of the hidden-sidebar controls", async () => {
    useUiStore.setState({ sidebarCollapsed: true });
    renderApp("/settings");
    const title = await h1("Settings");
    expect(title.closest("header")!.className).toContain("pl-[92px]");

    act(() => useUiStore.setState({ sidebarCollapsed: false }));
    await waitFor(() => expect(title.closest("header")!.className).not.toContain("pl-[92px]"));
  });

  it("shows a LIVE badge on Recording while capturing", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    // Set after mount: the shell re-syncs from the (mock) backend on mount, which reports idle.
    act(() => useRecordingStore.setState({ state: "recording", elapsed_seconds: 3 }));
    expect(await within(aside).findByText("LIVE")).toBeTruthy();
  });
});

describe("Redesign shell: downloads popover", () => {
  it("shows the empty state until a download is active", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    const trigger = within(aside).getByRole("button", { name: "Downloads" });
    expect(screen.queryByRole("dialog", { name: "Download progress" })).toBeNull();

    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Download progress" });
    expect(within(dialog).getByText(/will appear here/i)).toBeTruthy();
    expect(within(dialog).queryByRole("progressbar")).toBeNull();
  });

  it("lists ongoing downloads with progress and flags the icon", async () => {
    useUiStore.setState({ downloadStatus: { active: true, modelName: "Test Model 7B", progress: 42, etaSeconds: 10 } });
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    const trigger = within(aside).getByRole("button", { name: "Downloads" });
    expect(trigger.querySelector(".bg-primary")).not.toBeNull(); // activity dot

    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Download progress" });
    expect(within(dialog).getByText("Test Model 7B")).toBeTruthy();
    expect(within(dialog).getByRole("progressbar").getAttribute("aria-valuenow")).toBe("42");
  });

  it("closes on Escape", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("button", { name: "Downloads" }));
    expect(screen.getByRole("dialog", { name: "Download progress" })).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Download progress" })).toBeNull());
  });

  it("no longer seeds a fake download on a fresh start", async () => {
    renderApp("/");
    await screen.findByLabelText("Primary navigation");
    expect(screen.queryByText(/Llama 3.2 3B Instruct$/)).toBeNull();
    expect(useUiStore.getState().downloadStatus.active).toBe(false);
  });
});

describe("Redesign shell: header", () => {
  it("has no theme toggle (theme lives in Settings)", async () => {
    renderApp("/");
    await h1("Meetings");
    expect(screen.queryByRole("button", { name: /theme/i })).toBeNull();
  });

  it("shows the REC pill on every page while recording and links back to Record", async () => {
    renderApp("/models");
    await h1("Model Manager");
    act(() => useRecordingStore.setState({ state: "recording", elapsed_seconds: 65.4 }));
    expect(await screen.findByText("REC")).toBeTruthy();
    expect(screen.getByText("01:05")).toBeTruthy();
    fireEvent.click(screen.getByText("REC"));
    expect(await screen.findByText("Recording Live")).toBeTruthy();
  });
});

describe("Redesign shell: keyboard shortcuts keep working", () => {
  it("Ctrl/Cmd+N and +R open Record, +, opens Settings, +K opens Meetings", async () => {
    renderApp("/models");
    await h1("Model Manager");

    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(await h1("Capture Hub")).toBeTruthy();

    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(await h1("Settings")).toBeTruthy();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(await h1("Meetings")).toBeTruthy();

    fireEvent.keyDown(window, { key: "r", metaKey: true });
    expect(await h1("Capture Hub")).toBeTruthy();
  });
});

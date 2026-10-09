import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, cleanup } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import { useRecordingStore } from "../stores/recordingStore";

const ROUTES: [string, string | RegExp][] = [
  ["/", "Meetings"],
  ["/record", "Capture Hub"],
  ["/meeting/m-01", /Q3 Architecture/],
  ["/knowledge", "Knowledge Base"],
  ["/models", "Model Manager"],
  ["/settings", "Settings"],
];
const THEMES = ["light", "dark"] as const;

let errorSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  localStorage.clear();
  useRecordingStore.setState({ state: "idle", elapsed_seconds: 0 });
  useUiStore.setState({ sidebarCollapsed: false });
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
});
afterEach(() => {
  cleanup();
  errorSpy.mockRestore();
});

/** React's act() chatter is harness noise, not an app error. */
const realErrors = () =>
  errorSpy.mock.calls
    .map((args) => args.map(String).join(" "))
    .filter((msg) => !/not wrapped in act|inside a test was not wrapped/i.test(msg));

const accessibleName = (el: Element) =>
  (el.textContent ?? "").trim() || el.getAttribute("aria-label") || el.getAttribute("title") || "";

describe.each(THEMES)("Smoke (%s theme): every route mounts cleanly", (theme) => {
  it.each(ROUTES)("%s renders its page, the shell, and no errors", async (path, heading) => {
    useUiStore.getState().setTheme(theme);
    renderApp(path);

    expect(await screen.findByRole("heading", { level: 1, name: heading })).toBeTruthy();
    expect(screen.getByLabelText("Primary navigation")).toBeTruthy();
    expect(document.documentElement.getAttribute("data-theme")).toBe(theme);

    // The ErrorBoundary fallback never shows.
    expect(screen.queryByText(/Unable to load/i)).toBeNull();
    expect(screen.queryByText(/Something went wrong/i)).toBeNull();
    expect(realErrors()).toEqual([]);
  });

  it("an unknown URL falls back to the not-found view inside the shell", async () => {
    useUiStore.getState().setTheme(theme);
    renderApp("/does-not-exist");
    expect(await screen.findByLabelText("Primary navigation")).toBeTruthy();
    expect(screen.queryByText(/Something went wrong/i)).toBeNull();
    expect(realErrors()).toEqual([]);
  });
});

describe("Smoke: accessibility basics survive the redesign", () => {
  it.each(ROUTES)("%s: every button has an accessible name and every field a label", async (path, heading) => {
    renderApp(path);
    await screen.findByRole("heading", { level: 1, name: heading });
    // Let data-driven content settle.
    await new Promise((r) => setTimeout(r, 50));

    const unnamed = screen.queryAllByRole("button").filter((b) => !accessibleName(b));
    expect(unnamed.map((b) => b.outerHTML.slice(0, 120))).toEqual([]);

    const fields = Array.from(document.querySelectorAll("input:not([type=file]):not([type=checkbox]), select, textarea"));
    const unlabelled = fields.filter((el) => {
      const f = el as HTMLInputElement;
      return !(f.labels && f.labels.length) && !el.getAttribute("aria-label") && !el.getAttribute("aria-labelledby");
    });
    expect(unlabelled.map((f) => f.outerHTML.slice(0, 120))).toEqual([]);
  });

  it("the hidden sidebar leaves named, reachable controls (no unnamed icon rail)", async () => {
    useUiStore.setState({ sidebarCollapsed: true });
    renderApp("/");
    const toolbar = await screen.findByRole("toolbar", { name: "Sidebar controls" });
    const buttons = Array.from(toolbar.querySelectorAll("button"));
    expect(buttons.length).toBe(2);
    for (const b of buttons) expect(accessibleName(b)).toBeTruthy();
    expect(document.querySelector("aside")).toBeNull();
  });
});

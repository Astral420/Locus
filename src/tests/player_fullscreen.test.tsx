import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import React from "react";

const windowApi = vi.hoisted(() => ({
  setFullscreen: vi.fn(async (_value: boolean) => undefined),
  isFullscreen: vi.fn(async () => false),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => windowApi }));

import { MediaPlayer } from "../components/player/MediaPlayer";

const renderPlayer = () =>
  render(<MediaPlayer hasVideo={true} meetingTitle="Demo" durationSeconds={60} currentTime={0} onTimeUpdate={vi.fn()} />);
const button = (name: RegExp) => screen.getByRole("button", { name });
const region = () => screen.getByRole("region", { name: /media player/i });

beforeEach(() => {
  windowApi.setFullscreen.mockReset().mockResolvedValue(undefined);
  windowApi.isFullscreen.mockReset().mockResolvedValue(false);
});
afterEach(() => {
  cleanup();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("Video fullscreen in the desktop app (window-level; the webview has no element Fullscreen API)", () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  it("makes the app window fullscreen and the player fills it", async () => {
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => expect(windowApi.setFullscreen).toHaveBeenCalledWith(true));
    await waitFor(() => expect(button(/exit fullscreen/i)).toBeDefined());
    expect(region().className).toContain("fixed inset-0");
    // `relative` next to `fixed` wins in Tailwind's CSS and silently cancels fullscreen
    expect(region().className.split(/\s+/)).not.toContain("relative");
    expect(region().getAttribute("data-fullscreen")).toBe("true");
  });

  it("leaves fullscreen with the button and with Escape", async () => {
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => button(/exit fullscreen/i));
    fireEvent.click(button(/exit fullscreen/i));
    await waitFor(() => expect(windowApi.setFullscreen).toHaveBeenLastCalledWith(false));
    await waitFor(() => button(/enter fullscreen/i));

    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => button(/exit fullscreen/i));
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(windowApi.setFullscreen).toHaveBeenLastCalledWith(false));
    await waitFor(() => button(/enter fullscreen/i));
    expect(region().className).not.toContain("fixed inset-0");
  });

  it("does not claim to be fullscreen when the window refused (the old behaviour flipped anyway)", async () => {
    windowApi.setFullscreen.mockRejectedValue(new Error("permission denied"));
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => expect(windowApi.setFullscreen).toHaveBeenCalled());
    expect(button(/enter fullscreen/i)).toBeDefined();
    expect(region().className).not.toContain("fixed inset-0");
  });

  it("restores the window if the player goes away while fullscreen", async () => {
    const { unmount } = renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => button(/exit fullscreen/i));
    windowApi.setFullscreen.mockClear();
    unmount();
    expect(windowApi.setFullscreen).toHaveBeenCalledWith(false);
  });

  it("follows the window when it leaves fullscreen by itself (green button, Ctrl+Cmd+F)", async () => {
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => button(/exit fullscreen/i));
    windowApi.isFullscreen.mockResolvedValue(false);
    fireEvent(window, new Event("resize"));
    await waitFor(() => button(/enter fullscreen/i));
  });
});

describe("Video fullscreen in a plain browser (standard Fullscreen API)", () => {
  const original = { request: HTMLElement.prototype.requestFullscreen, exit: document.exitFullscreen };
  afterEach(() => {
    HTMLElement.prototype.requestFullscreen = original.request;
    document.exitFullscreen = original.exit;
    Object.defineProperty(document, "fullscreenElement", { value: null, configurable: true });
  });

  it("only flips state once the browser actually entered fullscreen", async () => {
    let entered: Element | null = null;
    HTMLElement.prototype.requestFullscreen = vi.fn(async function (this: HTMLElement) {
      entered = this;
    });
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => expect(HTMLElement.prototype.requestFullscreen).toHaveBeenCalled());
    expect(button(/enter fullscreen/i)).toBeDefined(); // not yet: no fullscreenchange has fired
    Object.defineProperty(document, "fullscreenElement", { value: entered, configurable: true });
    document.dispatchEvent(new Event("fullscreenchange"));
    await waitFor(() => button(/exit fullscreen/i));
    expect(windowApi.setFullscreen).not.toHaveBeenCalled();
  });

  it("stays put when the request is rejected", async () => {
    HTMLElement.prototype.requestFullscreen = vi.fn(async () => {
      throw new Error("not allowed");
    });
    renderPlayer();
    fireEvent.click(button(/enter fullscreen/i));
    await waitFor(() => expect(HTMLElement.prototype.requestFullscreen).toHaveBeenCalled());
    expect(button(/enter fullscreen/i)).toBeDefined();
  });
});

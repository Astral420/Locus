import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import React from "react";
import { useRecordingStore } from "../stores/recordingStore";
import { useUiStore } from "../stores/uiStore";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { StatusDot } from "../components/ui/StatusDot";
import { EmptyState } from "../components/ui/EmptyState";
import { Skeleton } from "../components/ui/Skeleton";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { RecoveryModal } from "../components/ui/RecoveryModal";
import { router } from "../router";
import { queryClient } from "../lib/queryClient";
import {
  listMeetings,
  listModels,
  listKnowledgeThreads,
  listDocuments,
  getRecordingState,
} from "../lib/tauri";

describe("M6.01: Design System & Tokens (Editorial Botanical)", () => {
  it("calculates contrast ratios exceeding WCAG AA/AAA standards", () => {
    // Luminance calculation helper
    function getRelativeLuminance(r: number, g: number, b: number) {
      const a = [r, g, b].map((v) => {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
    }

    function getContrast(rgb1: [number, number, number], rgb2: [number, number, number]) {
      const lum1 = getRelativeLuminance(...rgb1);
      const lum2 = getRelativeLuminance(...rgb2);
      const brightest = Math.max(lum1, lum2);
      const darkest = Math.min(lum1, lum2);
      return (brightest + 0.05) / (darkest + 0.05);
    }

    // Light Paper ground #FAF8F5 (250, 248, 245) vs Deep Botanical Pine Ink #19211D (25, 33, 29)
    const lightPaperContrast = getContrast([250, 248, 245], [25, 33, 29]);
    expect(lightPaperContrast).toBeGreaterThan(13.0); // 13.8:1 target
    expect(lightPaperContrast).toBeGreaterThan(4.5);  // Passes WCAG AA

    // Accessible Green Text #265C44 (38, 92, 68) on Paper ground #FAF8F5
    const greenTextContrast = getContrast([250, 248, 245], [38, 92, 68]);
    expect(greenTextContrast).toBeGreaterThan(6.0);   // 6.2:1 target
    expect(greenTextContrast).toBeGreaterThan(4.5);  // Passes WCAG AA

    // Neutral Graphite ground #121212 (18, 18, 18) vs off-white Ink #EDEDED (237, 237, 237)
    const darkSlateContrast = getContrast([18, 18, 18], [237, 237, 237]);
    expect(darkSlateContrast).toBeGreaterThan(14.0);  // 14.5:1 target
  });

  it("toggles theme correctly via useUiStore and sets data-theme on documentElement", () => {
    const { setTheme } = useUiStore.getState();
    setTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");

    setTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });
});

describe("M6.02: App Shell & Sidebar Specifications", () => {
  beforeEach(() => {
    useUiStore.setState({ sidebarCollapsed: false });
  });

  it("toggles sidebar collapsed state in uiStore", () => {
    const { toggleSidebar } = useUiStore.getState();
    expect(useUiStore.getState().sidebarCollapsed).toBe(false);

    toggleSidebar();
    expect(useUiStore.getState().sidebarCollapsed).toBe(true);

    toggleSidebar();
    expect(useUiStore.getState().sidebarCollapsed).toBe(false);
  });

  it("exposes active background download progress in uiStore", () => {
    // No fake download is seeded any more: the store starts idle.
    const { downloadStatus, setDownloadStatus } = useUiStore.getState();
    expect(downloadStatus.active).toBe(false);
    expect(downloadStatus.progress).toBe(0);

    setDownloadStatus({ active: true, modelName: "Test Model", progress: 80 });
    expect(useUiStore.getState().downloadStatus.progress).toBe(80);
    setDownloadStatus({ active: false, modelName: "", progress: 0 });
  });
});

describe("M6.03: TanStack Router Route Resolution", () => {
  it("registers all required routes with type-safe route trees", () => {
    const routeTree = router.routeTree;
    expect(routeTree).toBeDefined();

    // Verify all 6 routes exist in the router routes list
    const routes = Object.keys(router.routesById);
    expect(routes).toContain("/");
    expect(routes).toContain("/record");
    expect(routes).toContain("/meeting/$id");
    expect(routes).toContain("/knowledge");
    expect(routes).toContain("/models");
    expect(routes).toContain("/settings");
  });
});

describe("M6.04: TanStack Query & Zustand State Contracts", () => {
  it("fetches mock data for meetings, models, threads, and documents", async () => {
    const meetings = await listMeetings();
    expect(meetings.length).toBeGreaterThan(0);
    expect(meetings[0].title).toBe("Q3 Architecture & Offline RAG Sync");

    const models = await listModels();
    expect(models.length).toBeGreaterThan(0);
    expect(models.some((m) => m.role === "whisper")).toBe(true);
    expect(models.some((m) => m.role === "llm")).toBe(true);
    expect(models.some((m) => m.role === "embedding")).toBe(true);

    const threads = await listKnowledgeThreads();
    expect(threads.length).toBeGreaterThan(0);

    const docs = await listDocuments();
    expect(docs.length).toBeGreaterThan(0);
  });

  it("manages recording store state transitions and defaults per PRD FR1.5 & FR12.2", async () => {
    useRecordingStore.setState({
      state: "idle",
      elapsed_seconds: 0,
      selected_sources: ["system_audio", "screen"],
      only_me_mic: false,
      warning: false,
    });

    const store = useRecordingStore.getState();
    // Default capture sources: System Audio ON, Screen ON, Mic OFF
    expect(store.selected_sources).toContain("system_audio");
    expect(store.selected_sources).toContain("screen");
    expect(store.selected_sources).not.toContain("microphone");
    expect(store.only_me_mic).toBe(false);

    // Toggle microphone on
    store.toggleSource("microphone");
    expect(useRecordingStore.getState().selected_sources).toContain("microphone");

    // Toggle only_me_mic
    store.setOnlyMeMic(true);
    expect(useRecordingStore.getState().only_me_mic).toBe(true);

    // Start recording
    await useRecordingStore.getState().start();
    expect(useRecordingStore.getState().state).toBe("recording");

    // Pause recording
    await useRecordingStore.getState().pause();
    expect(useRecordingStore.getState().state).toBe("paused");

    // Resume recording
    await useRecordingStore.getState().resume();
    expect(useRecordingStore.getState().state).toBe("recording");

    // Stop recording
    await useRecordingStore.getState().stop();
    expect(useRecordingStore.getState().state).toBe("saved");
  });

  it("triggers 3-hour warning banner when recording elapsed exceeds 10800 seconds", () => {
    useRecordingStore.setState({
      state: "recording",
      elapsed_seconds: 10799,
      warning: false,
    });

    useRecordingStore.getState().tick();
    expect(useRecordingStore.getState().elapsed_seconds).toBe(10800);
    expect(useRecordingStore.getState().warning).toBe(true);
  });
});

describe("M6.05: View Scaffolds & Accessible States", () => {
  it("renders accessible Skeleton components with role='status' and aria-busy", () => {
    render(<Skeleton data-testid="test-skeleton" className="h-6 w-32" />);
    const el = screen.getByTestId("test-skeleton");
    expect(el.getAttribute("role")).toBe("status");
    expect(el.getAttribute("aria-busy")).toBe("true");
  });

  it("renders EmptyState with icon, title, description, and actionable CTA button", () => {
    let clicked = false;
    render(
      <EmptyState
        title="No recordings yet"
        description="Recorded sessions will appear here."
        actionLabel="Start a Recording"
        onAction={() => {
          clicked = true;
        }}
      />
    );

    expect(screen.getByText("No recordings yet")).toBeDefined();
    expect(screen.getByText("Recorded sessions will appear here.")).toBeDefined();

    const btn = screen.getByRole("button", { name: "Start a Recording" });
    fireEvent.click(btn);
    expect(clicked).toBe(true);
  });

  it("renders ErrorBoundary fallback with retry action when child component throws", () => {
    const ProblemChild: React.FC = () => {
      throw new Error("Crash in component");
    };

    render(
      <ErrorBoundary fallbackTitle="Custom View Error">
        <ProblemChild />
      </ErrorBoundary>
    );

    expect(screen.getByText("Custom View Error")).toBeDefined();
    expect(screen.getByText("Crash in component")).toBeDefined();
    expect(screen.getByRole("button", { name: /Retry View/i })).toBeDefined();
  });

  it("renders RecoveryModal with recoverable duration and explicit actions", () => {
    let recovered = false;
    let discarded = false;

    render(
      <RecoveryModal
        isOpen={true}
        onRecover={() => {
          recovered = true;
        }}
        onDiscard={() => {
          discarded = true;
        }}
        onClose={() => {}}
        sessionDate="Sep 26, 2026 at 13:42"
        durationFormatted="34m 12s"
      />
    );

    expect(screen.getByText("Interrupted Recording Detected")).toBeDefined();
    expect(screen.getByText("34m 12s")).toBeDefined();

    const recoverBtn = screen.getByRole("button", { name: "Recover and Process" });
    fireEvent.click(recoverBtn);
    expect(recovered).toBe(true);

    const discardBtn = screen.getByRole("button", { name: "Discard Recording" });
    fireEvent.click(discardBtn);
    expect(discarded).toBe(true);
  });
});

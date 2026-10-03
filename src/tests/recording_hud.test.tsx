import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import React from "react";

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn() }));

import { Header } from "../components/layout/Header";
import { useRecordingStore } from "../stores/recordingStore";

describe("Header REC pill", () => {
  beforeEach(() => {
    useRecordingStore.setState({ state: "idle", elapsed_seconds: 0 });
  });

  it("renders whole-second MM:SS for fractional backend elapsed time (regression: 00:5.617268483)", () => {
    useRecordingStore.setState({ state: "recording", elapsed_seconds: 5.617268483 });
    render(<Header title="x" />);
    expect(screen.getByText("REC")).toBeTruthy();
    expect(screen.getByText("00:05")).toBeTruthy();
    expect(screen.queryByText(/\./)).toBeNull();
  });

  it("rolls over to hours like the Capture Hub timer", () => {
    useRecordingStore.setState({ state: "recording", elapsed_seconds: 3725.7 });
    render(<Header title="x" />);
    expect(screen.getByText("01:02:05")).toBeTruthy();
  });

  it("stays visible and says PAUSED while paused", () => {
    useRecordingStore.setState({ state: "paused", elapsed_seconds: 12.9 });
    render(<Header title="x" />);
    expect(screen.getByText("PAUSED")).toBeTruthy();
    expect(screen.getByText("00:12")).toBeTruthy();
  });

  it("is hidden when idle", () => {
    render(<Header title="x" />);
    expect(screen.queryByText("REC")).toBeNull();
  });
});

describe("recording store", () => {
  it("syncFromBackend keeps selected sources and tolerates a failing backend", async () => {
    useRecordingStore.setState({ selected_sources: ["microphone"] });
    await useRecordingStore.getState().syncFromBackend();
    expect(useRecordingStore.getState().selected_sources.length).toBeGreaterThan(0);
  });
});

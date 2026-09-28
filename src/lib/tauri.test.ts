import { describe, expect, it } from "vitest";
import type { RecordingStateDTO } from "./tauri";

describe("shared contracts", () => {
  it("keeps recording state fields serializable for Tauri", () => {
    const state: RecordingStateDTO = { capture_id: null, meeting_id: null, state: "idle", generation: 0, elapsed_seconds: 0, selected_sources: [], recoverable: false, reason: null, warning: false };
    expect(JSON.parse(JSON.stringify(state))).toEqual(state);
  });
});

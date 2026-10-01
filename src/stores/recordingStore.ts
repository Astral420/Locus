import { create } from "zustand";
import {
  type CaptureLifecycle,
  type CaptureSource,
  type MeetingType,
  type RecordingStateDTO,
  getRecordingState,
  startRecording,
  pauseRecording,
  resumeRecording,
  stopRecording,
} from "../lib/tauri";

interface RecordingStoreState {
  state: CaptureLifecycle;
  capture_id: string | null;
  meeting_id: string | null;
  meeting_title: string;
  meeting_type: MeetingType;
  elapsed_seconds: number;
  selected_sources: CaptureSource[];
  only_me_mic: boolean;
  system_audio_level: number; // dBFS, -90 = silence
  mic_level: number;          // dBFS, -90 = silence
  recoverable: boolean;
  warning: boolean;
  reason: string | null;

  // Actions
  setTitle: (title: string) => void;
  setType: (type: MeetingType) => void;
  toggleSource: (source: CaptureSource) => void;
  setOnlyMeMic: (onlyMe: boolean) => void;
  tick: () => void;
  dismissWarning: () => void;
  start: () => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  syncFromBackend: () => Promise<void>;
}

export const useRecordingStore = create<RecordingStoreState>((set, get) => ({
  state: "idle",
  capture_id: null,
  meeting_id: null,
  meeting_title: "Untitled Conversation",
  meeting_type: "auto",
  elapsed_seconds: 0,
  // PRD FR1.5 defaults: System Audio (ON) + Screen (ON) + Microphone (OFF)
  selected_sources: ["system_audio", "screen"],
  only_me_mic: false,
  system_audio_level: -90,
  mic_level: -90,
  recoverable: false,
  warning: false,
  reason: null,

  setTitle: (title) => set({ meeting_title: title }),
  setType: (type) => set({ meeting_type: type }),

  toggleSource: (source) => {
    const current = get().selected_sources;
    const exists = current.includes(source);
    const updated = exists ? current.filter((s) => s !== source) : [...current, source];
    set({ selected_sources: updated });
  },

  setOnlyMeMic: (onlyMe) => set({ only_me_mic: onlyMe }),

  tick: () => {
    const { state, elapsed_seconds } = get();
    if (state === "recording") {
      const nextSeconds = Math.floor(elapsed_seconds) + 1;
      // 3-hour warning threshold (10800 seconds) per PRD FR12.5
      const showWarning = nextSeconds >= 10800;
      set({ elapsed_seconds: nextSeconds, warning: showWarning });
    }
  },

  dismissWarning: () => set({ warning: false }),

  start: async () => {
    const { selected_sources, meeting_type } = get();
    // Validate: at least one audio source required
    const hasAudio = selected_sources.includes("system_audio") || selected_sources.includes("microphone");
    if (!hasAudio) {
      throw new Error("At least one audio source (System Audio or Microphone) is required to start recording.");
    }
    const res = await startRecording(selected_sources, meeting_type, get().meeting_title, get().only_me_mic);
    set({
      state: res.state,
      capture_id: res.capture_id,
      meeting_id: res.meeting_id,
      elapsed_seconds: res.elapsed_seconds,
      selected_sources: res.selected_sources,
      recoverable: res.recoverable,
      warning: res.warning,
      reason: res.reason,
      system_audio_level: res.system_audio_level ?? -90,
      mic_level: res.mic_level ?? -90,
    });
  },

  pause: async () => {
    const res = await pauseRecording();
    set({ state: res.state });
  },

  resume: async () => {
    const res = await resumeRecording();
    set({ state: res.state });
  },

  stop: async () => {
    const res = await stopRecording();
    set({
      state: res.state,
      system_audio_level: res.system_audio_level ?? -90,
      mic_level: res.mic_level ?? -90,
      capture_id: null,
      meeting_id: null,
      elapsed_seconds: 0,
      warning: false,
    });
  },

  syncFromBackend: async () => {
    try {
      const res: RecordingStateDTO = await getRecordingState();
      set({
        state: res.state,
        capture_id: res.capture_id,
        meeting_id: res.meeting_id,
        elapsed_seconds: res.elapsed_seconds,
        selected_sources: res.selected_sources.length > 0 ? res.selected_sources : get().selected_sources,
        recoverable: res.recoverable,
        warning: res.warning,
        reason: res.reason,
        system_audio_level: res.system_audio_level ?? -90,
        mic_level: res.mic_level ?? -90,
      });
    } catch (err) {
      console.warn("Failed to sync recording state from backend:", err);
    }
  },
}));

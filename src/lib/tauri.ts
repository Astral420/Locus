import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export function safeConvertFileSrc(filePath: string): string {
  if (isTauriEnvironment()) {
    try {
      return convertFileSrc(filePath);
    } catch {
      return filePath;
    }
  }
  return filePath;
}

export type MeetingType = "auto" | "meeting" | "lecture";
export type CaptureLifecycle =
  | "idle"
  | "preparing"
  | "recording"
  | "paused"
  | "finalizing"
  | "saved"
  | "interrupted"
  | "recoverable";
export type CaptureSource = "system_audio" | "microphone" | "screen";

export interface MeetingDTO {
  id: string;
  title: string;
  title_origin: "placeholder" | "automatic" | "user";
  title_revision: number;
  recorded_at: string;
  timezone: string;
  duration_seconds: number;
  requested_type: MeetingType;
  detected_type: "meeting" | "lecture" | "generic" | null;
  lifecycle: string;
  deleted_at: string | null;
  speaker_count?: number;
}

export interface MeetingMediaDTO {
  /** Absolute path of the playable recording, or null when none was captured. */
  path: string | null;
  has_video: boolean;
  duration_seconds: number;
}

export interface RecordingStateDTO {
  capture_id: string | null;
  meeting_id: string | null;
  state: CaptureLifecycle;
  generation: number;
  elapsed_seconds: number;
  selected_sources: CaptureSource[];
  recoverable: boolean;
  reason: string | null;
  warning: boolean;
  system_audio_level?: number;
  mic_level?: number;
}

export interface PipelineStatusDTO {
  job_id: string;
  kind: string;
  state: "pending" | "running" | "done" | "error" | "blocked" | "skipped" | "canceled";
  progress: number;
  reason: string | null;
}

export interface ModelAssetDTO {
  id: string;
  name: string;
  role: "whisper" | "llm" | "embedding";
  parameter_size: string;
  quantization?: string;
  memory_ram: string;
  memory_vram: string;
  disk_size: string;
  status: "bundled" | "installed" | "active" | "downloading" | "available" | "error";
  download_progress?: number;
  // Optional catalog fields (e.g. from a GitHub-hosted models.json). The Model Manager renders each only when present.
  author?: string;
  description?: string;
  downloads?: number;
  tags?: string[];
  /** Whether the model fits this machine's memory, when the catalog or hardware probe knows. */
  fits?: boolean;
  variants?: ModelVariantDTO[];
}

export interface ModelVariantDTO {
  quantization: string;
  disk_size: string;
  memory_ram?: string;
  url?: string;
}

export interface StorageInfoDTO {
  data_root: string;
  models_root: string;
  free_bytes: number | null;
  capture_active: boolean;
  migration_state: string | null;
}

export interface GpuBackendDTO {
  backend: string;
  architecture: string;
  device: string | null;
  reason: string;
  vulkan_ready: boolean;
}

export interface UpdateCheckDTO {
  channel: string;
  current_version: string;
  available_version: string | null;
  package_url: string | null;
  sha256: string | null;
  verified: boolean;
}

export interface TranscriptSegmentDTO {
  id: string;
  speaker_id: string;
  speaker_label: string;
  start_time: number;
  end_time: number;
  text: string;
}

export interface SlideDTO {
  id: string;
  ordinal: number;
  timestamp: number;
  image_url: string;
  ocr_text: string;
  repeated_timestamps?: number[];
}

export interface ActionItemDTO {
  id: string;
  summary_revision_id: string;
  text: string;
  assignee: string | null;
  deadline: string | null;
  completed: boolean;
}

export interface SummaryRevisionDTO {
  id: string;
  revision_number: number;
  meeting_id: string;
  overview: string;
  decisions: string[];
  key_concepts: string[];
  created_at: string;
  model: string;
  is_outdated?: boolean;
}

export interface SpeakerPaletteColor {
  label: string;
  light: { text: string; bg: string; border: string };
  dark: { text: string; bg: string; border: string };
}

export const DIARIZATION_PALETTE: SpeakerPaletteColor[] = [
  // Speaker 1: Emerald
  {
    label: "Emerald",
    light: { text: "#265C44", bg: "#E2F3EB", border: "#B6DFC9" },
    dark: { text: "#64D6A2", bg: "#173629", border: "#23533E" },
  },
  // Speaker 2: Slate Blue
  {
    label: "Slate Blue",
    light: { text: "#2B5B84", bg: "#E3EDF5", border: "#B8D3E8" },
    dark: { text: "#78B3E4", bg: "#1A2F42", border: "#2A4864" },
  },
  // Speaker 3: Ochre Rust
  {
    label: "Ochre Rust",
    light: { text: "#7A4B27", bg: "#F5ECE3", border: "#E5CFBC" },
    dark: { text: "#E09A64", bg: "#3D2717", border: "#5C3B24" },
  },
  // Speaker 4: Plum
  {
    label: "Plum",
    light: { text: "#6B3D6B", bg: "#F3E8F3", border: "#E1C6E1" },
    dark: { text: "#C688C6", bg: "#351D35", border: "#522D52" },
  },
  // Speaker 5: Teal
  {
    label: "Teal",
    light: { text: "#2C6661", bg: "#E2F0EF", border: "#B4D9D7" },
    dark: { text: "#6CC7BF", bg: "#163330", border: "#244E4A" },
  },
  // Speaker 6: Warm Olive
  {
    label: "Warm Olive",
    light: { text: "#73561E", bg: "#F4EFE0", border: "#E2D6B7" },
    dark: { text: "#D6AE56", bg: "#3A2B0F", border: "#5A4318" },
  },
  // Speaker 7+: Neutral Steel
  {
    label: "Neutral Steel",
    light: { text: "#525866", bg: "#EAECEF", border: "#D0D5DD" },
    dark: { text: "#A3ABB8", bg: "#252830", border: "#3B404D" },
  },
];

export function getSpeakerPalette(speakerIdOrIndex: string | number, isDark = false): { text: string; bg: string; border: string; hue: string } {
  let index = 0;
  if (typeof speakerIdOrIndex === "number") {
    index = speakerIdOrIndex;
  } else {
    const match = speakerIdOrIndex.match(/\d+/);
    if (match) {
      index = parseInt(match[0], 10) - 1;
    } else {
      let hash = 0;
      for (let i = 0; i < speakerIdOrIndex.length; i++) {
        hash = (hash << 5) - hash + speakerIdOrIndex.charCodeAt(i);
        hash |= 0;
      }
      index = Math.abs(hash);
    }
  }
  const colorIndex = Math.min(Math.max(0, index), DIARIZATION_PALETTE.length - 1);
  const palette = DIARIZATION_PALETTE[colorIndex];
  const themeColors = isDark ? palette.dark : palette.light;
  return {
    ...themeColors,
    hue: palette.label,
  };
}

export type ProviderKind = "llama_server" | "ollama" | "openai" | "anthropic" | "gemini";

export interface ProviderSnapshotDTO {
  provider: ProviderKind;
  model: string;
  destination: string;
  selected_at: string;
  disclosure: string;
}

export interface ProviderConfigDTO {
  provider: ProviderKind;
  display_name: string;
  model: string;
  destination: string;
  configured: boolean;
  selected: boolean;
}

export interface SummaryActionItemDTO {
  id: string;
  summary_revision_id: string;
  text: string;
  assignee_speaker_id: string | null;
  deadline_phrase: string | null;
  normalized_deadline: string | null;
  evidence_refs: unknown[];
  completed: boolean;
}

export interface BackendSummaryRevisionDTO {
  id: string;
  meeting_id: string;
  revision: number;
  meeting_type: "meeting" | "lecture" | "generic";
  structured_json: Record<string, unknown>;
  markdown: string;
  input_revision_set: unknown[];
  model_identity: string | null;
  prompt_identity: string | null;
  freshness: "current" | "outdated";
  published_at: string | null;
  action_items: SummaryActionItemDTO[];
}

export type KnowledgeScope = "this_meeting" | "all_meetings" | "documents_only" | "everything";

export interface KnowledgeThreadDTO {
  id: string;
  title: string;
  scope: KnowledgeScope;
  meeting_id?: string | null;
  updated_at: string;
  message_count: number;
}

export interface DocumentDTO {
  id: string;
  filename: string;
  file_size: string;
  size_bytes?: number;
  page_count: number;
  media_type?: string;
  status: "ready" | "indexing" | "error";
  uploaded_at: string;
  extraction_error?: string | null;
}

export interface KnowledgeCitationDTO {
  id: string;
  source_chunk_id: string;
  source_kind: string;
  source_title: string;
  location: Record<string, unknown>;
  start_seconds: number | null;
  end_seconds: number | null;
  page_number: number | null;
  text_start: number | null;
  text_end: number | null;
}

export interface KnowledgeMessageDTO {
  id: string;
  thread_id: string;
  ordinal: number;
  role: "user" | "assistant" | "system";
  content: string;
  state: "pending" | "complete" | "error" | "canceled";
  provider_identity: string | null;
  model_identity: string | null;
  created_at: string;
  citations: KnowledgeCitationDTO[];
}

export interface KnowledgeSearchResultDTO {
  chunk_id: string;
  source_id: string;
  source_kind: string;
  source_title: string;
  text: string;
  distance: number;
  start_seconds: number | null;
  end_seconds: number | null;
  page_number: number | null;
  text_start: number | null;
  text_end: number | null;
}

export interface KnowledgeIndexStatusDTO {
  generation_id: string | null;
  embedding_model_id: string | null;
  state: "blocked" | "building" | "ready" | "active" | "error";
  ready_sources: number;
  pending_sources: number;
  error_sources: number;
}

export interface EmbeddingModelDTO {
  id: string;
  display_name: string;
  filename: string;
  size_bytes: number;
  ram_bytes: number;
  dimension: number;
  installed: boolean;
  selected: boolean;
  setup_skipped: boolean;
}

// Helpers for mock fallbacks when running outside Tauri or in testing
export function isTauriEnvironment(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Recording commands must never silently fall back to mock data inside the real
 * app: a failed start would otherwise look like a live recording that captures
 * nothing. Backend errors arrive as plain strings, so normalise them to Error.
 */
async function invokeStrict<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  if (!isTauriEnvironment()) {
    return fallback();
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    if (err instanceof Error) throw err;
    throw new Error(typeof err === "string" ? err : JSON.stringify(err));
  }
}

async function safeInvoke<T>(cmd: string, args?: Record<string, unknown>, fallback?: () => T): Promise<T> {
  if (isTauriEnvironment()) {
    try {
      return await invoke<T>(cmd, args);
    } catch (err) {
      if (fallback) {
        return fallback();
      }
      throw err;
    }
  }
  if (fallback) {
    return fallback();
  }
  throw new Error(`Tauri command '${cmd}' unavailable in non-Tauri environment`);
}

// Mock seed data for development & browser testing
export const MOCK_MEETINGS: MeetingDTO[] = [
  {
    id: "m-01",
    title: "Q3 Architecture & Offline RAG Sync",
    title_origin: "automatic",
    title_revision: 1,
    recorded_at: "2026-09-10T14:00:00Z",
    timezone: "America/Los_Angeles",
    duration_seconds: 2535,
    requested_type: "meeting",
    detected_type: "meeting",
    lifecycle: "saved",
    deleted_at: null,
    speaker_count: 3,
  },
  {
    id: "m-02",
    title: "Distributed Systems & Consensus Lecture",
    title_origin: "user",
    title_revision: 2,
    recorded_at: "2026-09-08T10:30:00Z",
    timezone: "America/Los_Angeles",
    duration_seconds: 3600,
    requested_type: "lecture",
    detected_type: "lecture",
    lifecycle: "saved",
    deleted_at: null,
    speaker_count: 2,
  },
  {
    id: "m-03",
    title: "Product Design Review — Editorial Botanical",
    title_origin: "automatic",
    title_revision: 1,
    recorded_at: "2026-09-05T16:15:00Z",
    timezone: "America/Los_Angeles",
    duration_seconds: 1820,
    requested_type: "meeting",
    detected_type: "meeting",
    lifecycle: "saved",
    deleted_at: null,
    speaker_count: 4,
  },
];

let mockMeetingsStore = [...MOCK_MEETINGS];

export const MOCK_SEGMENTS: TranscriptSegmentDTO[] = [
  {
    id: "seg-1",
    speaker_id: "spk-1",
    speaker_label: "Speaker 1",
    start_time: 868, // 14:28
    end_time: 872,
    text: "The architecture aligns with an offline-first desktop environment without central database reliance.",
  },
  {
    id: "seg-2",
    speaker_id: "spk-1",
    speaker_label: "Speaker 1",
    start_time: 872, // 14:32 (ACTIVE)
    end_time: 879,
    text: "We converted the Python sidecar to an on-demand process with ChromaDB vector indexing running as a background job.",
  },
  {
    id: "seg-3",
    speaker_id: "spk-2",
    speaker_label: "Speaker 2",
    start_time: 880, // 14:40
    end_time: 886,
    text: "Does that run on CPU fallback when discrete GPU acceleration is unavailable?",
  },
  {
    id: "seg-4",
    speaker_id: "spk-1",
    speaker_label: "Speaker 1",
    start_time: 887,
    end_time: 894,
    text: "Yes, whisper-rs and llama.cpp both fall back cleanly to CPU instructions with bounded memory guarantees.",
  },
  {
    id: "seg-5",
    speaker_id: "spk-3",
    speaker_label: "Speaker 3",
    start_time: 900,
    end_time: 912,
    text: "Let's ensure the crash recovery manifest retains independent decodable segments with at most 5 seconds of tail loss.",
  },
];

let mockSegmentsStore = [...MOCK_SEGMENTS];

export const MOCK_SLIDES: SlideDTO[] = [
  {
    id: "sld-1",
    ordinal: 1,
    timestamp: 130, // 02:10
    image_url: "/slides/slide_1.png",
    ocr_text: "System Architecture: Tauri Rust Core + Python Sidecar RPC",
  },
  {
    id: "sld-2",
    ordinal: 2,
    timestamp: 525, // 08:45
    image_url: "/slides/slide_2.png",
    ocr_text: "Data Boundaries: SQLite for relational state, ChromaDB for vectors",
  },
  {
    id: "sld-3",
    ordinal: 3,
    timestamp: 870, // 14:30
    image_url: "/slides/slide_3.png",
    ocr_text: "Memory Constraints: Bounded inference RAM, lazy sidecar lifecycle",
    repeated_timestamps: [1695], // also at 28:15
  },
  {
    id: "sld-4",
    ordinal: 4,
    timestamp: 1335, // 22:15
    image_url: "/slides/slide_4.png",
    ocr_text: "Pipeline Sequencing: Balanced vs Maximum parallelism modes",
  },
  {
    id: "sld-5",
    ordinal: 5,
    timestamp: 1800, // 30:00
    image_url: "/slides/slide_5.png",
    ocr_text: "Platform Matrix: macOS Apple Silicon / Intel AMD Vulkan, Windows, Linux",
  },
];

export const MOCK_SUMMARY_REVISIONS: SummaryRevisionDTO[] = [
  {
    id: "sum-rev-2",
    revision_number: 2,
    meeting_id: "m-01",
    overview:
      "The engineering team confirmed the offline-first desktop architecture for Locus v1. Key agreements focused on strict RAM bounds by executing ML inference on-demand and separating ChromaDB vector indexing into background tasks.",
    decisions: [
      "Sidecar starts lazily and runs ChromaDB indexing in the background [14:32].",
      "Audio track routing diarizes both microphone and system audio streams anonymously unless personal mic setting is enabled [08:45].",
      "Whisper small remains bundled as the default offline model with zero network token requirements [02:10].",
    ],
    key_concepts: [
      "Precision Master Control Room Ergonomics",
      "VAD-guided 5-minute chunking with lowest-energy boundary split",
      "Revision-owned action item completion state",
    ],
    created_at: "2026-09-10T15:10:00Z",
    model: "llama-3.2-3b",
    is_outdated: false,
  },
  {
    id: "sum-rev-1",
    revision_number: 1,
    meeting_id: "m-01",
    overview:
      "Initial draft discussion on desktop container and memory footprints for local Whisper and Llama models.",
    decisions: [
      "Target Apple Silicon and Intel discrete GPUs with bounded 8GB host fixtures.",
      "Preserve previous summary revisions upon regeneration without transferring checkmarks.",
    ],
    key_concepts: [
      "Local Keychain Auth",
      "Deterministic Vector Chunk IDs",
    ],
    created_at: "2026-09-10T14:45:00Z",
    model: "llama-3.2-3b",
    is_outdated: true,
  },
];

export const MOCK_SUMMARY: SummaryRevisionDTO = MOCK_SUMMARY_REVISIONS[0];

export const MOCK_ACTIONS: ActionItemDTO[] = [
  {
    id: "act-1",
    summary_revision_id: "sum-rev-2",
    text: "Package standalone Python sidecar binary with PyInstaller",
    assignee: "Speaker 1",
    deadline: "Friday, Oct 2",
    completed: true,
  },
  {
    id: "act-2",
    summary_revision_id: "sum-rev-2",
    text: "Benchmark whisper-rs transcription throughput on 8GB RAM fixture",
    assignee: "Speaker 2",
    deadline: "Monday, Oct 5",
    completed: false,
  },
  {
    id: "act-3",
    summary_revision_id: "sum-rev-2",
    text: "Draft durable crash manifest format for segment recovery",
    assignee: null,
    deadline: null,
    completed: false,
  },
];

let mockActionsStore = [...MOCK_ACTIONS];

export const MOCK_PIPELINE_STATUS: PipelineStatusDTO[] = [
  { job_id: "j-encode", kind: "recording_encoding", state: "done", progress: 100, reason: null },
  { job_id: "j-vad", kind: "vad", state: "done", progress: 100, reason: null },
  { job_id: "j-transcribe", kind: "transcription", state: "done", progress: 100, reason: null },
  { job_id: "j-diarize", kind: "diarization", state: "done", progress: 100, reason: null },
  { job_id: "j-slides", kind: "slides_and_ocr", state: "done", progress: 100, reason: null },
  { job_id: "j-summary", kind: "summarization", state: "done", progress: 100, reason: null },
];

let mockPipelineStatusStore = [...MOCK_PIPELINE_STATUS];

const MOCK_MODELS: ModelAssetDTO[] = [
  {
    id: "whisper-small",
    name: "Whisper Small (Bundled)",
    role: "whisper",
    parameter_size: "244M",
    memory_ram: "1.0 GB",
    memory_vram: "1.0 GB",
    disk_size: "466 MiB",
    status: "active",
  },
  {
    id: "whisper-medium",
    name: "Whisper Medium",
    role: "whisper",
    parameter_size: "769M",
    memory_ram: "2.6 GB",
    memory_vram: "2.6 GB",
    disk_size: "1.5 GB",
    status: "available",
  },
  {
    id: "whisper-large-v3",
    name: "Whisper Large v3",
    role: "whisper",
    parameter_size: "1550M",
    memory_ram: "4.8 GB",
    memory_vram: "4.8 GB",
    disk_size: "3.1 GB",
    status: "available",
  },
  {
    id: "llama-3.2-3b",
    name: "Llama 3.2 3B Instruct (GGUF)",
    role: "llm",
    parameter_size: "3.2B",
    quantization: "Q4_K_M",
    memory_ram: "3.5 GB",
    memory_vram: "3.0 GB",
    disk_size: "2.0 GB",
    status: "active",
    author: "Meta",
    description: "A compact 3B instruction-tuned model that runs comfortably on CPU and is well suited to meeting summaries.",
    downloads: 289502,
    tags: ["Tools"],
    fits: true,
    variants: [
      { quantization: "Q4_K_M", disk_size: "2.0 GB", memory_ram: "3.2 GB" },
      { quantization: "Q8_0", disk_size: "3.4 GB", memory_ram: "4.6 GB" },
    ],
  },
  {
    id: "mistral-7b-instruct",
    name: "Mistral 7B Instruct v0.3",
    role: "llm",
    parameter_size: "7.2B",
    quantization: "Q4_K_M",
    memory_ram: "6.0 GB",
    memory_vram: "5.5 GB",
    disk_size: "4.3 GB",
    status: "available",
    author: "Mistral AI",
    description: "A 7B instruction model with stronger reasoning for long, technical meetings. Needs more memory.",
    downloads: 25734,
    tags: ["Tools", "Long context"],
    fits: false,
    variants: [
      { quantization: "Q4_K_M", disk_size: "4.4 GB", memory_ram: "6.2 GB" },
      { quantization: "Q5_K_M", disk_size: "5.1 GB", memory_ram: "7.0 GB" },
    ],
  },
  {
    id: "bge-small-en",
    name: "BGE Small English v1.5",
    role: "embedding",
    parameter_size: "33M",
    memory_ram: "500 MB",
    memory_vram: "500 MB",
    disk_size: "133 MiB",
    status: "active",
  },
];

const MOCK_THREADS: KnowledgeThreadDTO[] = [
  {
    id: "th-1",
    title: "Sidecar Memory Architecture Decisions",
    scope: "all_meetings",
    updated_at: "2026-09-10T15:30:00Z",
    message_count: 4,
  },
  {
    id: "th-2",
    title: "Raft Consensus Invariants from Lecture",
    scope: "this_meeting",
    updated_at: "2026-09-08T12:00:00Z",
    message_count: 6,
  },
];

const MOCK_DOCUMENTS: DocumentDTO[] = [
  {
    id: "doc-1",
    filename: "arch_v2_specification.pdf",
    file_size: "2.4 MB",
    page_count: 28,
    status: "ready",
    uploaded_at: "2026-09-09T11:00:00Z",
  },
  {
    id: "doc-2",
    filename: "spec_draft.md",
    file_size: "48 KB",
    page_count: 4,
    status: "ready",
    uploaded_at: "2026-09-07T09:20:00Z",
  },
];

export function greet(name: string): Promise<string> {
  return safeInvoke("greet", { name }, () => `Hello, ${name}! Locus Engine Ready.`);
}

export function getRecordingState(): Promise<RecordingStateDTO> {
  return safeInvoke("get_recording_state", undefined, () => ({
    capture_id: null,
    meeting_id: null,
    state: "idle",
    generation: 0,
    elapsed_seconds: 0,
    selected_sources: ["system_audio", "screen"],
    recoverable: false,
    reason: null,
    warning: false,
  }));
}

export function showRecordingWindow(): Promise<void> {
  return safeInvoke("show_recording_window", undefined, () => undefined);
}

/** Best-effort: pre-loads FFmpeg/display info so Record starts faster. Never throws. */
export function prewarmCapture(): Promise<void> {
  return invokeStrict<void>("prewarm_capture", undefined, () => undefined).catch(() => undefined);
}

export function startRecording(
  sources: CaptureSource[],
  meetingType: MeetingType = "auto",
  title = "",
  singlePersonMic = false,
): Promise<RecordingStateDTO> {
  return invokeStrict("start_recording", { sources, meetingType, title, singlePersonMic }, () => ({
    capture_id: "cap-live-01",
    meeting_id: "m-live-01",
    state: "recording",
    generation: 1,
    elapsed_seconds: 0,
    selected_sources: sources,
    recoverable: false,
    reason: null,
    warning: false,
    system_audio_level: sources.includes("system_audio") ? -12 : undefined,
    mic_level: sources.includes("microphone") ? -48 : undefined,
  }));
}

export function pauseRecording(): Promise<RecordingStateDTO> {
  return invokeStrict("pause_recording", undefined, () => ({
    capture_id: "cap-live-01",
    meeting_id: "m-live-01",
    state: "paused",
    generation: 1,
    elapsed_seconds: 12,
    selected_sources: ["system_audio", "screen"],
    recoverable: false,
    reason: null,
    warning: false,
    system_audio_level: -12,
    mic_level: -48,
  }));
}

export function resumeRecording(): Promise<RecordingStateDTO> {
  return invokeStrict("resume_recording", undefined, () => ({
    capture_id: "cap-live-01",
    meeting_id: "m-live-01",
    state: "recording",
    generation: 1,
    elapsed_seconds: 12,
    selected_sources: ["system_audio", "screen"],
    recoverable: false,
    reason: null,
    warning: false,
  }));
}

export function stopRecording(): Promise<RecordingStateDTO> {
  return invokeStrict("stop_recording", undefined, () => ({
    capture_id: null,
    meeting_id: null,
    state: "saved",
    generation: 1,
    elapsed_seconds: 24,
    selected_sources: ["system_audio", "screen"],
    recoverable: false,
    reason: null,
    warning: false,
  }));
}

export function listMeetings(): Promise<MeetingDTO[]> {
  return safeInvoke("list_meetings", undefined, () => mockMeetingsStore);
}

export function getMeeting(id: string): Promise<MeetingDTO | null> {
  return safeInvoke("get_meeting", { id }, () => {
    const found = mockMeetingsStore.find((m) => m.id === id);
    return found || null;
  });
}

export function getMeetingMedia(meetingId: string): Promise<MeetingMediaDTO | null> {
  return safeInvoke("get_meeting_media", { meetingId }, () => null);
}

export function deleteMeeting(id: string): Promise<void> {
  return safeInvoke("delete_meeting", { meetingId: id }, () => {
    mockMeetingsStore = mockMeetingsStore.filter((m) => m.id !== id);
  });
}

export function getPipelineStatus(meetingId: string): Promise<PipelineStatusDTO[]> {
  return safeInvoke("get_pipeline_status", { meetingId }, () => mockPipelineStatusStore);
}

export function retryPipelineStep(jobId: string): Promise<void> {
  return safeInvoke("retry_pipeline_step", { jobId }, () => {
    mockPipelineStatusStore = mockPipelineStatusStore.map((job) =>
      job.job_id === jobId ? { ...job, state: "pending", progress: 0, reason: null } : job
    );
  });
}

export function listTranscriptSegments(meetingId: string): Promise<TranscriptSegmentDTO[]> {
  return safeInvoke("list_transcript_segments", { meetingId }, () => mockSegmentsStore);
}

export function renameSpeaker(speakerId: string, displayName: string): Promise<void> {
  return safeInvoke("rename_speaker", { speakerId, displayName }, () => {
    mockSegmentsStore = mockSegmentsStore.map((s) =>
      s.speaker_id === speakerId ? { ...s, speaker_label: displayName } : s
    );
  });
}

export function getSlides(meetingId: string): Promise<SlideDTO[]> {
  return safeInvoke("get_slides", { meetingId }, () => MOCK_SLIDES);
}

export function listModels(): Promise<ModelAssetDTO[]> {
  return safeInvoke("list_models", undefined, () => MOCK_MODELS);
}

export function selectModel(modelId: string, role: ModelAssetDTO["role"]): Promise<void> {
  return safeInvoke("select_model", { modelId, role }, () => undefined);
}

export function getGpuBackend(): Promise<GpuBackendDTO> {
  return safeInvoke("get_gpu_backend", undefined, () => ({
    backend: "cpu",
    architecture: "browser",
    device: null,
    reason: "Browser preview has no native GPU runtime.",
    vulkan_ready: false,
  }));
}

export function getStorageInfo(): Promise<StorageInfoDTO> {
  return safeInvoke("get_storage_info", undefined, () => ({
    data_root: "~/Library/Application Support/Locus/data",
    models_root: "~/Library/Application Support/Locus/models",
    free_bytes: null,
    capture_active: false,
    migration_state: null,
  }));
}

export function migrateModels(targetPath: string): Promise<string> {
  return safeInvoke("migrate_models", { targetPath });
}

export function relocateStorage(targetPath: string): Promise<string> {
  return safeInvoke("relocate_storage", { targetPath });
}

export function checkForUpdate(endpoint?: string, currentVersion = "0.1.0"): Promise<UpdateCheckDTO> {
  return safeInvoke("check_for_update", { endpoint: endpoint ?? "", currentVersion }, () => ({
    channel: "stable",
    current_version: currentVersion,
    available_version: null,
    package_url: null,
    sha256: null,
    verified: false,
  }));
}

export function listKnowledgeThreads(): Promise<KnowledgeThreadDTO[]> {
  return safeInvoke("list_knowledge_threads", undefined, () => MOCK_THREADS);
}

export function listDocuments(): Promise<DocumentDTO[]> {
  return safeInvoke("list_documents", undefined, () => MOCK_DOCUMENTS);
}

const MOCK_KNOWLEDGE_MESSAGES: KnowledgeMessageDTO[] = [
  {
    id: "km-1",
    thread_id: "th-1",
    ordinal: 0,
    role: "user",
    content: "What decisions were made regarding the sidecar memory?",
    state: "complete",
    provider_identity: null,
    model_identity: null,
    created_at: "2026-09-10T15:20:00Z",
    citations: [],
  },
  {
    id: "km-2",
    thread_id: "th-1",
    ordinal: 1,
    role: "assistant",
    content:
      "The team agreed to keep the Python sidecar on demand and run ChromaDB indexing as a background job.",
    state: "complete",
    provider_identity: "llama_server",
    model_identity: "llama-3.2-3b",
    created_at: "2026-09-10T15:21:00Z",
    citations: [
      {
        id: "cite-1",
        source_chunk_id: "chunk-1",
        source_kind: "transcript",
        source_title: "Q3 Architecture & Offline RAG Sync",
        location: { start_seconds: 872, end_seconds: 879 },
        start_seconds: 872,
        end_seconds: 879,
        page_number: null,
        text_start: null,
        text_end: null,
      },
    ],
  },
];

let mockKnowledgeMessages = [...MOCK_KNOWLEDGE_MESSAGES];

export function listKnowledgeMessages(threadId: string): Promise<KnowledgeMessageDTO[]> {
  return safeInvoke(
    "list_knowledge_messages",
    { threadId },
    () => mockKnowledgeMessages.filter((message) => message.thread_id === threadId)
  );
}

export function createKnowledgeThread(
  scope: KnowledgeScope,
  meetingId?: string | null
): Promise<KnowledgeThreadDTO> {
  return safeInvoke(
    "create_knowledge_thread",
    { input: { scope, meeting_id: meetingId ?? null } },
    () => ({
      id: "th-" + Date.now(),
      title: "New Knowledge inquiry",
      scope,
      meeting_id: meetingId ?? null,
      updated_at: new Date().toISOString(),
      message_count: 0,
    })
  );
}

export function sendKnowledgeMessage(
  threadId: string,
  content: string
): Promise<KnowledgeMessageDTO> {
  return safeInvoke("send_knowledge_message", { threadId, content }, () => {
    const assistant: KnowledgeMessageDTO = {
      id: "km-" + Date.now(),
      thread_id: threadId,
      ordinal: mockKnowledgeMessages.filter((message) => message.thread_id === threadId).length,
      role: "assistant",
      content:
        "I found a grounded answer in the selected local sources. The supporting evidence is shown below.",
      state: "complete",
      provider_identity: "llama_server",
      model_identity: "llama-3.2-3b",
      created_at: new Date().toISOString(),
      citations: [
        {
          id: "cite-" + Date.now(),
          source_chunk_id: "chunk-1",
          source_kind: "transcript",
          source_title: "Q3 Architecture & Offline RAG Sync",
          location: { start_seconds: 872, end_seconds: 879 },
          start_seconds: 872,
          end_seconds: 879,
          page_number: null,
          text_start: null,
          text_end: null,
        },
      ],
    };
    mockKnowledgeMessages = [
      ...mockKnowledgeMessages,
      {
        id: "user-" + Date.now(),
        thread_id: threadId,
        ordinal: assistant.ordinal - 1,
        role: "user",
        content,
        state: "complete",
        provider_identity: null,
        model_identity: null,
        created_at: new Date().toISOString(),
        citations: [],
      },
      assistant,
    ];
    return assistant;
  });
}

export function uploadDocument(
  filename: string,
  mediaType: string,
  bytes: ArrayBuffer,
  meetingId?: string | null
): Promise<{ document: DocumentDTO; duplicate: boolean }> {
  const raw = new Uint8Array(bytes);
  let binary = "";
  const chunkSize = 0x8000;
  for (let index = 0; index < raw.length; index += chunkSize) {
    binary += String.fromCharCode(...raw.subarray(index, index + chunkSize));
  }
  const contentBase64 = btoa(binary);
  return safeInvoke(
    "upload_document",
    {
      input: {
        filename,
        media_type: mediaType,
        content_base64: contentBase64,
        meeting_id: meetingId ?? null,
      },
    },
    () => ({
      document: {
        id: "doc-" + Date.now(),
        filename,
        file_size: String(Math.max(1, Math.round(bytes.byteLength / 1024))) + " KB",
        size_bytes: bytes.byteLength,
        page_count: filename.toLowerCase().endsWith(".pdf") ? 1 : 0,
        media_type: mediaType,
        status: "indexing",
        uploaded_at: new Date().toISOString(),
        extraction_error: null,
      },
      duplicate: false,
    })
  );
}

export function searchKnowledge(
  scope: KnowledgeScope,
  query: string,
  meetingId?: string | null,
  limit = 10
): Promise<KnowledgeSearchResultDTO[]> {
  return safeInvoke(
    "search_knowledge",
    { scope, meetingId: meetingId ?? null, query, limit },
    () => [
      {
        chunk_id: "chunk-1",
        source_id: "source-1",
        source_kind: "transcript",
        source_title: "Q3 Architecture & Offline RAG Sync",
        text: "The Python sidecar runs on demand and ChromaDB indexing is independent.",
        distance: 0.12,
        start_seconds: 872,
        end_seconds: 879,
        page_number: null,
        text_start: null,
        text_end: null,
      },
    ]
  );
}

export function getKnowledgeIndexStatus(): Promise<KnowledgeIndexStatusDTO> {
  return safeInvoke("get_knowledge_index_status", undefined, () => ({
    generation_id: "mock-generation",
    embedding_model_id: "bge-small-en-v1.5",
    state: "active",
    ready_sources: 6,
    pending_sources: 0,
    error_sources: 0,
  }));
}

export function indexKnowledgeSource(sourceId: string): Promise<KnowledgeIndexStatusDTO> {
  return safeInvoke("index_knowledge_source", { sourceId }, () => ({
    generation_id: "mock-generation",
    embedding_model_id: "bge-small-en-v1.5",
    state: "active",
    ready_sources: 6,
    pending_sources: 0,
    error_sources: 0,
  }));
}

export function retryKnowledgeIndex(sourceId: string): Promise<void> {
  return safeInvoke("retry_knowledge_index", { sourceId });
}

export function listEmbeddingModels(): Promise<EmbeddingModelDTO[]> {
  return safeInvoke("list_embedding_models", undefined, () => [
    {
      id: "bge-small-en-v1.5",
      display_name: "BGE Small English v1.5",
      filename: "bge-small-en-v1.5.Q8_0.gguf",
      size_bytes: 133 * 1024 * 1024,
      ram_bytes: 500 * 1024 * 1024,
      dimension: 384,
      installed: true,
      selected: true,
      setup_skipped: false,
    },
  ]);
}

export function skipEmbeddingSetup(): Promise<void> {
  return safeInvoke("skip_embedding_setup", undefined, () => undefined);
}

export function configureProvider(
  provider: ProviderKind,
  model: string,
  destination: string,
  configured = true
): Promise<void> {
  return safeInvoke("configure_provider", { provider, model, destination, configured });
}

export function saveProviderApiKey(provider: ProviderKind, apiKey: string): Promise<void> {
  return safeInvoke("save_provider_api_key", { provider, apiKey });
}

export function selectProvider(
  provider: ProviderKind,
  model: string,
  destination: string,
  acceptDisclosure: boolean
): Promise<ProviderSnapshotDTO> {
  return safeInvoke("select_provider", {
    input: { provider, model, destination, accept_disclosure: acceptDisclosure },
  });
}

export function listProviderConfigs(): Promise<ProviderConfigDTO[]> {
  return safeInvoke("get_provider_configs", undefined, () => []);
}

export function triggerSummary(meetingId: string, regenerate = false): Promise<BackendSummaryRevisionDTO> {
  return safeInvoke("trigger_summary", { meetingId, regenerate });
}

export function listSummaryRevisions(meetingId: string): Promise<SummaryRevisionDTO[]> {
  return safeInvoke("get_summary_revisions", { meetingId }, () => MOCK_SUMMARY_REVISIONS);
}

export function listActionItems(summaryRevisionId: string): Promise<ActionItemDTO[]> {
  return safeInvoke("get_action_items", { summaryRevisionId }, () =>
    mockActionsStore.filter((a) => a.summary_revision_id === summaryRevisionId)
  );
}

export function toggleActionItem(actionItemId: string, completed: boolean): Promise<void> {
  return safeInvoke("toggle_action_item", { actionItemId, completed }, () => {
    mockActionsStore = mockActionsStore.map((item) =>
      item.id === actionItemId ? { ...item, completed } : item
    );
  });
}

export function setMeetingType(meetingId: string, meetingType: MeetingType): Promise<void> {
  return safeInvoke("set_meeting_type", { meetingId, meetingType });
}

export function setMeetingTitle(meetingId: string, title: string): Promise<void> {
  return safeInvoke("set_meeting_title", { meetingId, title });
}

export function retrySummary(meetingId: string): Promise<BackendSummaryRevisionDTO> {
  return safeInvoke("retry_summary", { meetingId });
}

// Helpers for formatted export (M8.09)
export function exportMeetingAsMarkdown(
  meeting: MeetingDTO,
  summary: SummaryRevisionDTO,
  actionItems: ActionItemDTO[],
  segments: TranscriptSegmentDTO[]
): string {
  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  const lines: string[] = [];
  lines.push(`# ${meeting.title}`);
  lines.push(`**Date:** ${new Date(meeting.recorded_at).toLocaleDateString()} · **Duration:** ${formatTime(meeting.duration_seconds)} · **Type:** ${meeting.detected_type || meeting.requested_type}`);
  lines.push("");
  lines.push("## Executive Overview");
  lines.push(summary.overview);
  lines.push("");
  lines.push("## Key Decisions");
  summary.decisions.forEach((dec) => lines.push(`- ${dec}`));
  lines.push("");
  lines.push("## Key Concepts");
  summary.key_concepts.forEach((concept) => lines.push(`- ${concept}`));
  lines.push("");
  lines.push("## Action Items");
  if (actionItems.length === 0) {
    lines.push("_No action items assigned._");
  } else {
    actionItems.forEach((act) => {
      const check = act.completed ? "[x]" : "[ ]";
      const assignee = act.assignee ? ` (@${act.assignee})` : "";
      const deadline = act.deadline ? ` [Due: ${act.deadline}]` : "";
      lines.push(`- ${check} ${act.text}${assignee}${deadline}`);
    });
  }
  lines.push("");
  lines.push("## Full Transcript");
  segments.forEach((seg) => {
    lines.push(`**[${formatTime(seg.start_time)}] ${seg.speaker_label}:** ${seg.text}`);
  });

  return lines.join("\n");
}

export function exportMeetingAsJson(
  meeting: MeetingDTO,
  summary: SummaryRevisionDTO,
  actionItems: ActionItemDTO[],
  segments: TranscriptSegmentDTO[],
  slides: SlideDTO[]
): string {
  return JSON.stringify(
    {
      meeting,
      summary,
      action_items: actionItems,
      transcript: segments,
      slides,
      exported_at: new Date().toISOString(),
    },
    null,
    2
  );
}

export function exportMeetingAsTxt(
  meeting: MeetingDTO,
  segments: TranscriptSegmentDTO[]
): string {
  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  const lines = [
    `TRANSCRIPT: ${meeting.title}`,
    `Recorded: ${meeting.recorded_at}`,
    "--------------------------------------------------",
    ...segments.map((seg) => `[${formatTime(seg.start_time)}] ${seg.speaker_label}: ${seg.text}`),
  ];
  return lines.join("\n");
}

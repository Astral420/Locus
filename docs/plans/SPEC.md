# Spec: Locus v1 — Offline-First Meeting Recording & Summarization

`ready-for-agent`

---

## Problem Statement

Knowledge workers and students attend meetings, lectures, and discussions daily, but capturing actionable information from them is manual and error-prone. Existing solutions are either cloud-only (privacy concerns, internet dependency), proprietary (vendor lock-in), or only cover part of the workflow (transcription without summarization, or summarization without recording). There is no lightweight, cross-platform, offline-first tool that handles the full loop — recording a meeting, transcribing it with speaker identification, extracting presentation slides, and generating context-aware summaries with action items and deadlines — all locally on the user's machine.

## Solution

Locus is an offline-first desktop application (macOS + Windows + Linux) that records meetings (system audio, microphone, and/or screen), transcribes them with speaker diarization, extracts slide content from screen recordings, and generates AI-powered summaries tailored to the meeting type (business meeting vs. academic lecture). A built-in Knowledge Base allows users to upload supporting documents and semantically search across all meetings and documents via RAG-powered chat. All processing runs locally by default using whisper.cpp for transcription, pyannote for speaker identification and voice activity detection, OpenCV for slide detection, ChromaDB for vector storage, and Ollama for summarization. The pipeline is designed for recordings up to 3 hours, using VAD-guided chunking for optimal performance. Users who prefer cloud AI can optionally configure API keys for OpenAI, Anthropic or Gemini. Linux support targets modern PipeWire-enabled distributions. The app stores everything in a local SQLite database (with ChromaDB for vector embeddings) and never sends data off-device unless the user explicitly chooses a cloud LLM provider.

## User Stories

### Recording

1. As a knowledge worker, I want to start recording a meeting with one click, so that I can capture meeting content without switching between apps.

2. As a user, I want to choose which audio/video sources to capture (system audio, microphone, screen) via toggle switches before recording, so that I only capture what's relevant for this particular meeting.

3. As a user, I want to name my meeting before or during recording, so that I can find it easily later.

4. As a user, I want to see a live timer and audio level indicator during recording, so that I know the recording is working and capturing audio.

5. As a user, I want to pause and resume recording, so that I can skip breaks or irrelevant portions of a meeting.

6. As a user, I want to stop recording and have the app automatically encode the raw capture into an MP4 file, so that I have a standard video file I can play back.

7. As a user, I want to select the meeting type (Auto-detect, Meeting, or Lecture) before recording, so that the AI summarization uses the right prompt for my context.

### Transcription

8. As a user, I want my recording to be automatically transcribed after it finishes, so that I get a text version of everything that was said without manual effort.

9. As a user, I want the transcription to include timestamps for each segment, so that I can jump to specific moments in the recording.

10. As a user, I want the app to auto-detect the spoken language, so that I don't have to configure it manually for every meeting.

11. As a user, I want to manually specify the language before recording when I know it in advance, so that I can improve transcription accuracy for my specific language.

12. As a user, I want to select which Whisper model to use (small, medium, large-v3), so that I can trade off between speed and accuracy based on my hardware.

13. As a user, I want to download larger Whisper models from within the app, so that I can upgrade transcription quality without leaving the application.

14. As a user, I want the app to automatically use my GPU (Metal on Apple Silicon, CUDA on NVIDIA, ROCm on AMD, Vulkan on Intel Mac with AMD GPU) for transcription, so that processing is as fast as possible on my hardware.

15. As a user with no supported GPU, I want transcription to fall back to CPU, so that the app still works on any machine.

### Diarization

16. As a user, I want the transcription to label which speaker said what, so that I can follow the conversation structure in multi-person meetings.

17. As a user, I want each speaker to have a distinct color in the transcript view, so that I can visually distinguish between speakers at a glance.

18. As a user, I want to rename auto-assigned speaker labels (e.g., "Speaker 1" → "Alice"), so that the transcript and summary reference people by name.

### Slide Extraction

19. As a user who recorded a screen with a presentation, I want the app to automatically detect slide transitions and extract each unique slide as an image, so that I can review presentation content alongside the transcript.

20. As a user, I want extracted slides to have timestamps linking them to the recording, so that I can click a slide and jump to that point in the video.

21. As a user, I want the text content of each slide to be extracted via OCR, so that slide content is searchable and can be included in the AI summary.

### Summarization

22. As a knowledge worker, I want the app to generate a summary with action items and deadlines after my meeting is processed, so that I know exactly what to do next without re-watching the recording.

23. As a student, I want the app to generate study notes with key concepts and assignments from my lecture, so that I have organized notes without manual note-taking.

24. As a user, I want the app to automatically detect whether my recording is a business meeting or a lecture and use the appropriate summarization prompt, so that the output format matches my context.

25. As a user, I want to override the auto-detected meeting type and re-run summarization, so that I get the right output format if auto-detection was wrong.

26. As a user, I want to connect to my local Ollama instance for summarization, so that I can run everything offline without cloud dependencies.

27. As a user, I want to provide an API key for OpenAI, Anthropic or Gemini as an alternative to local LLM, so that I can get higher-quality summaries if I prefer cloud AI.

28. As a user, I want to mark action items as completed, so that I can track my progress on meeting follow-ups.

### Meeting Management

29. As a user, I want to see a list of all my past recordings with title, date, duration, speaker count, and meeting type, so that I can browse my meeting history.

30. As a user, I want to search and filter my meetings by title, type, and date range, so that I can quickly find a specific recording.

31. As a user, I want to view a meeting's recording, transcript, summary, and slides in a single detail view, so that all meeting content is in one place.

32. As a user, I want the video player to sync with the transcript — clicking a transcript line seeks the video, and the current line highlights as the video plays — so that I can navigate the recording by content.

33. As a user, I want to see the processing status of each pipeline step (encoding, transcription, diarization, slide extraction, summarization), so that I know what's done and what's still running.

34. As a user, I want to retry a failed pipeline step without re-running the entire pipeline, so that a diarization failure doesn't force me to re-transcribe.

35. As a user, I want to export my meeting's transcript and summary as Markdown, PDF, or JSON, so that I can share or archive meeting content outside the app.

36. As a user, I want to copy the summary to my clipboard with one click, so that I can quickly paste it into Slack, email, or a document.

### Settings & Configuration

37. As a user, I want to store my API keys securely in my OS keychain (macOS Keychain / Windows Credential Manager), so that my credentials are protected by the operating system's encryption.

38. As a user, I want the app to automatically check for updates and offer to install them, so that I stay on the latest version. I also want to be able to download updates manually from GitHub Releases if I'm offline.

### Knowledge Base

39. As a user, I want to upload supporting documents (PDF, Markdown, plain text) to the Knowledge Base, so that I can enrich the context available for AI-powered search and chat.

40. As a user, I want to semantically search across all my meetings and documents by meaning (not just keywords), so that I can find relevant discussions without remembering exact words.

41. As a user, I want to chat with the Knowledge Base ("What did we decide about the pricing model?"), so that I can get AI-powered answers grounded in my actual meeting transcripts and documents.

42. As a user, I want to ask questions about a specific meeting from the meeting detail view, so that I can quickly find information within that recording's context.

43. As a user, I want to link uploaded documents to specific meetings, so that the AI can use those documents as additional context when answering questions about that meeting.

44. As a user, I want embedding generation to happen automatically in the background after summarization, so that new meetings are searchable without manual action.

45. As a user with a 3-hour recording, I want the pipeline to use voice activity detection to skip silence and chunk audio intelligently, so that transcription completes in reasonable time without degraded accuracy.

## Implementation Decisions

### Architecture

- **Desktop shell**: Tauri 2.11 with a React 19+ frontend rendered in the platform webview. Rust handles all backend orchestration, recording capture, transcription, database access, and process management.
- **Python sidecar**: A PyInstaller-packaged standalone binary.
  - **Sidecar responsibilities**: pyannote (VAD + diarization), OpenCV headless (slide detection), Tesseract (OCR), ChromaDB (vector storage for Knowledge Base).
  - **Sidecar lifecycle**: On-demand with warm-up. The sidecar starts lazily on first use (first pipeline run or first KB query) and stays running while the app is open. First interaction has a ~3-5 second cold start. Models are loaded on demand.
  - **OpenCV**: Uses `opencv-python-headless` to reduce sidecar size (no GUI dependencies).
- **Communication protocol**: JSON-RPC 2.0 over Tauri's sidecar stdin/stdout pipes. Rust sends typed requests (`diarize`, `detect_slides`, `ocr_slide`), Python responds with results. Python can emit progress notifications (JSON-RPC notifications without `id`).
- **Single monorepo** with clean module boundaries: `src-tauri/` (Rust), `src/` (React), `sidecar/` (Python). Each Rust module has a single responsibility: `capture/`, `transcription/`, `pipeline/`, `sidecar/`, `llm/`, `db/`, `commands/`. The `commands/` layer is thin and delegates to domain modules.

### Recording

- **Capture**: Rust-native via ScreenCaptureKit (macOS), Windows Graphics Capture API (Windows), and PipeWire portal (Linux). Platform-specific modules behind a shared trait.
- **Linux capture**: PipeWire screen cast portal for Wayland screen capture and PipeWire audio capture for system audio. PipeWire-only — no PulseAudio or X11 fallback. Minimum: Ubuntu 22.04+, Fedora 34+, Arch Linux.
- **Encoding**: Rust spawns a bundled ffmpeg binary as a subprocess. ffmpeg is included in the application bundle — no user installation required. Raw audio/video streams from the capture API are piped to ffmpeg for H.264 + AAC encoding into MP4.
- **Separate audio tracks**: System audio and microphone are captured as independent audio streams, muxed into a stereo (or multi-track) MP4 during encoding. Tracks are available separately for downstream processing.
- **User-selected sources**: Three independent toggles — System Audio, Microphone, Screen. Default is System Audio + Screen enabled.

### Transcription

- **Engine**: whisper-rs Rust bindings (from codeberg.org/tazz4843/whisper-rs) for whisper.cpp. Transcription runs in the Rust process with zero Python involvement.
- **Voice Activity Detection**: pyannote VAD runs in the Python sidecar on the mixed audio track before transcription. Identifies speech regions and outputs time ranges. Silence is skipped during transcription.
- **Audio chunking**: VAD-guided variable chunks with a 5-minute maximum. Contiguous speech regions are grouped into chunks; if a speech region exceeds 5 minutes (e.g., continuous lecture), it is split at the lowest-energy point within the region. Whisper processes each chunk independently. Timestamps are aligned to the original recording timeline.
- **GPU backends** with runtime detection:
  - macOS Apple Silicon → Metal + CoreML
  - macOS Intel + AMD GPU → Vulkan + MoltenVK
  - Windows/Linux NVIDIA → CUDA
  - Windows/Linux AMD → ROCm / HIP
  - Fallback → CPU on all platforms
- **Model management**: The `small` model (~250MB) is bundled with the app for immediate out-of-box experience. An in-app model manager allows downloading `tiny`, `base`, `medium`, and `large-v3`. Models are stored in the app data directory. The UI shows estimated RAM/VRAM requirements per model.
- **Quantized models**: The model manager offers both standard GGML weights and quantized variants (q5_0, q5_1, q8_0) for each Whisper model. Standard weights are the default. Quantized variants are 30-50% smaller with minor accuracy tradeoffs, making larger models accessible on constrained hardware.
- **Language**: Whisper auto-detects by default. A language dropdown in the recording setup allows manual override.
- **Timing**: Post-recording only. Transcription runs after recording stops. The pipeline is designed and tested for recordings up to 3 hours using VAD-guided chunking.

### Diarization

- **Engine**: pyannote-audio running in the Python sidecar.
- **Model**: `pyannote/speaker-diarization-community-1` is bundled with the PyInstaller binary. No HuggingFace token required at runtime.
- **Integration**: Rust sends the audio file path to the sidecar via JSON-RPC. Python runs pyannote, returns speaker-labeled time segments. Rust merges these with whisper-rs transcript segments to produce speaker-attributed text.

### Slide Extraction

- **Detection**: OpenCV frame differencing in the Python sidecar. Runs as a post-recording batch job over the MP4 video. Detects slide transitions by comparing frame similarity, extracts unique slide frames as images.
- **OCR**: Tesseract (via `pytesseract`) extracts text from each slide image. Bundled with PyInstaller.
- **Storage**: Slide images saved to disk. Metadata (timestamp, image path, OCR text) stored in SQLite.

### Summarization

- **LLM providers**: Ollama (auto-detected at `localhost:11434`) and cloud APIs (OpenAI, Anthropic) via user-provided API keys. No bundled LLM runtime — Locus does not ship its own inference engine.
- **Prompt routing**: Keyword heuristics on the transcript text automatically detect meeting type ("lecture" keywords → academic prompt, "sprint/deadline" keywords → business prompt, uncertain → generic prompt). User can override the detected type before or after summarization, triggering a re-run with the correct prompt template.
- **Output**: Summary markdown, extracted action items (with optional deadlines and assignees), and key decisions. Stored in SQLite linked to the meeting.

### Knowledge Base

- **Vector store**: ChromaDB running in the Python sidecar. Persists embeddings and metadata to disk. The sidecar manages ChromaDB lifecycle.
- **Embedding model**: EmbeddingGemma-300M, run in the Rust process via ONNX Runtime (`ort` crate). The model (~600MB) is bundled with the app for immediate offline use. Embeddings are generated in Rust and batch-sent to the sidecar via JSON-RPC for ChromaDB storage.
- **Embedding pipeline**: After summarization completes, transcript chunks, the summary, and any linked documents are embedded as a silent background process. Toast notifications inform the user of status and errors. This is not a formal pipeline step — it runs asynchronously and does not block the pipeline status UI.
- **Document upload**: Users can upload PDF (text extraction via Rust `pdf-extract` crate), Markdown, and plain text files. Documents are chunked, embedded, and stored in ChromaDB.
- **Document-meeting linking**: Documents exist independently in the Knowledge Base. Users can optionally tag documents to one or more meetings, providing additional RAG context for meeting-scoped queries.
- **Knowledge Base view** (`/knowledge`): Hybrid design — default view shows a list of all documents and meetings with their embedding status. A semantic search bar enables meaning-based search across all content. A "Chat" button opens a slide-in chat panel for RAG-powered Q&A. Scope toggles: "This meeting", "All meetings", "Documents only", "Everything".
- **Per-meeting Q&A**: The meeting detail view includes an "Ask about this meeting" input that performs scoped RAG retrieval using the meeting's transcript, summary, and linked documents.

### Pipeline

- **Pipeline steps**: Encode → VAD → Transcribe → Diarize & Slide Detection → OCR → Summarize. (Embedding runs silently in the background after Summarize).
- **Pipeline parallelism**: Configurable (default: parallel). After encoding, VAD and Slide Detection run concurrently. After VAD → Transcription completes, Diarization and OCR run concurrently. Summarization waits for both. Embedding runs silently after summarization. Users can switch to sequential mode ("Balanced") for resource-constrained machines via a setting.
- **Audio track routing**: Mixed audio (mic + system combined) goes to VAD and transcription. Separate audio tracks go to diarization — the mic track is auto-assigned as "User" (known single speaker), and the system audio track goes through pyannote's full speaker identification pipeline.

### Database

- **Engine**: SQLite via `rusqlite` or `sqlx`. Single-file database in the app data directory. Vector embeddings stored in ChromaDB (separate from SQLite).
- **Schema**: 9 tables — `meetings`, `pipeline_steps`, `transcript_segments`, `speakers`, `slides`, `summaries`, `action_items`, `documents`, `document_meetings`, plus a `settings` key-value table. Indexed on meeting foreign keys and timestamps.
- **Credentials**: API keys stored in the OS keychain via the `keyring` Rust crate (macOS Keychain, Windows Credential Manager, Linux Secret Service). NOT in SQLite.

### Frontend

- **UI framework**: Astryx (Meta's design system, 150+ components) with TailwindCSS for style overrides. UI design work uses the impeccable skill.
- **State management**: TanStack Query for server state (async data from Tauri commands), Zustand for client state (recording UI, active tabs).
- **Routing**: TanStack Router with type-safe routes.
- **Video playback**: Plyr or Video.js library wrapping an HTML5 `<video>` element. Transcript sync via `onTimeUpdate` events bidirectionally linked to transcript line highlighting and click-to-seek.
- **Views (v1)**: Home/Meeting List (`/`), Recording (`/record`), Meeting Detail (`/meeting/:id`), Settings (`/settings`).
- **Export**: Clipboard copy, Markdown file, PDF, JSON — all triggered from the Meeting Detail view.

### Error Handling

- **Granular per-step pipeline status**: Each processing step (encode, transcribe, diarize, slides, OCR, summarize) has independent status tracking (`pending → running → done → error`) in the `pipeline_steps` table.
- **Partial results preserved**: If diarization fails, the undiarized transcript is still available. If the LLM is unavailable, transcript and slides are still viewable. Each failed step is independently retriable.
- **Progress reporting**: The Python sidecar emits JSON-RPC notifications with progress percentages. The frontend polls pipeline step statuses via TanStack Query.

### Distribution

- **License**: Fully open source — MIT or Apache 2.0.
- **Platforms**: macOS (universal binary — Apple Silicon + Intel, DMG + Homebrew), Windows (separate builds for CUDA, ROCm, CPU-only, exe installer), Linux (AppImage + Flatpak for PipeWire-enabled distros).
- **Auto-update**: Tauri's built-in updater checks GitHub Releases for new versions. Manual download always available as fallback.
- **Bundled assets**: ffmpeg binary, whisper `small` model, pyannote community diarization model, Tesseract OCR, Python sidecar binary, EmbeddingGemma-300M model, ChromaDB.

## Testing Decisions

### What makes a good test

A good test for Locus exercises **external behavior through the defined seams**, not internal implementation details. Tests should verify that given a known input (an audio/video file), the system produces the expected output (transcript segments with speaker labels, extracted slides, a summary). Tests should not assert on internal data structures, intermediate pipeline state, or the specific sequence of internal function calls. If you can refactor the internals and the tests still pass, the tests are good.

### Seam 1: Pipeline Orchestrator (primary)

- **What it tests**: The full processing pipeline from "MP4 file exists" to "transcript, speakers, slides, and summary are in SQLite."
- **How**: Feed the orchestrator a pre-recorded audio/video fixture file with known content (e.g., a 2-minute clip with 2 speakers discussing a known topic, with a slide transition). Assert that:
  - Transcript segments exist and contain expected phrases
  - Speaker labels are assigned (at least 2 distinct speakers detected)
  - Slide images are extracted at expected timestamps
  - OCR text from slides matches expected content
  - Summary contains expected action items / key points
  - Pipeline step statuses are all `done`
  - Partial failure scenario: mock the LLM to fail, assert transcript and slides are still persisted, summary step is `error`, other steps are `done`
- **LLM mocking**: For deterministic tests, the LLM provider is swapped with a mock that returns a known summary. Integration tests against real Ollama can run in CI with a dedicated job.
- **Fixtures**: Small audio/video files (2-3 minutes) committed to `tests/fixtures/`. One with multiple speakers, one with slides, one with a single speaker (to test graceful diarization).

### Seam 2: JSON-RPC Protocol Boundary

- **What it tests**: The serialization contract between Rust and Python.
- **How**: 
  - Rust-side: Unit tests that serialize JSON-RPC requests and deserialize responses, verifying the protocol format matches the spec.
  - Python-side: `pytest` tests that feed JSON-RPC request strings to the RPC server's handler function and assert the response structure (correct method dispatch, proper error codes for unknown methods, progress notification format).
- **Why this seam exists**: The process boundary between Rust and Python is a common failure point (type mismatches, field name changes, encoding issues). Catching these at the protocol level produces clear error messages rather than cryptic runtime failures in Seam 1.

### Not tested automatically

- **Recording capture**: ScreenCaptureKit / Windows Graphics Capture require actual hardware, OS permissions, and active audio sources. These are tested manually per platform during release QA.
- **GPU backend selection**: Runtime detection of Metal/CUDA/ROCm/Vulkan depends on hardware. CI runs CPU-only. GPU backends are tested manually on target hardware.
- **Frontend UI**: Not tested in v1. UI will evolve rapidly. Visual regression testing and component tests are a v2 concern once the design stabilizes.

### Test tooling

- **Rust**: `cargo test` with `#[tokio::test]` for async pipeline tests
- **Python**: `pytest` for sidecar RPC handler tests
- **Fixtures**: Committed to `tests/fixtures/` — small, deterministic audio/video files
- **CI**: GitHub Actions — runs `cargo test` (CPU-only backend) and `pytest` on every push

## Out of Scope

The following are explicitly **not** in v1 and are deferred to future versions:

- **Real-time transcription** (v2): Live transcription during recording. v1 is post-recording only.
- **Dashboard & Calendar views** (v3): Overview stats, deadline tracking, timeline view of meetings.
- **Speaker voice profiles** (v3): Auto-identifying returning speakers across meetings by voice fingerprint.
- **Custom prompt template editor** (v3): User-editable summarization prompt templates.
- **Mobile companion app** (v3): Mobile viewing of summaries and transcripts.
- **In-person / ambient recording** (future): Recording via device microphone for in-person meetings. v1 focuses on system audio capture from video calls.
- **Meeting scheduling integration** (future): Calendar integration (Google Calendar, Outlook) for auto-naming or auto-triggering recordings.
- **Collaboration / team features** (future): Sharing meetings, team workspaces, shared knowledge bases.
- **Frontend component/visual tests**: Deferred until the design language stabilizes.

## Further Notes

### Key Risks

1. **PyInstaller bundle size**: Bundling PyTorch + pyannote + OpenCV + Tesseract will produce a sidecar binary in the 2-4 GB range. This is the largest contributor to install size. Mitigation: Accept ~4-4.5GB install size. Use OpenCV headless to reduce size. No lite build — sidecar is required for core functionality (VAD, diarization, ChromaDB).

2. **Vulkan + MoltenVK on Intel Macs**: Using Vulkan via MoltenVK to access AMD GPUs on Intel Macs for whisper.cpp acceleration is theoretically possible but not widely tested. This path needs prototyping early. Fallback to CPU is always available.

3. **pyannote model licensing**: The `speaker-diarization-community-1` model is under a permissive license, but the terms should be reviewed by the maintainer before bundling in an MIT/Apache-licensed application. The model weights are distributed under their own license separate from the pyannote code.

4. **ffmpeg licensing**: ffmpeg itself is LGPL/GPL. Bundling a build with only LGPL-compatible codecs avoids GPL contamination. Using platform hardware encoders (VideoToolbox on macOS, Media Foundation on Windows) through ffmpeg's LGPL-compatible API is the safe path. This needs verification during build configuration.

5. **Cross-platform recording capture**: ScreenCaptureKit (macOS 12.3+) and Windows Graphics Capture have different capabilities and permission models. Abstracting them behind a shared Rust trait is architecturally clean but the implementations will diverge significantly. This is the most platform-specific code in the entire app and the hardest to test.

6. **PipeWire requirement on Linux**: Linux support requires PipeWire. Users on PulseAudio-only systems (older Ubuntu, older Fedora) cannot record. Mitigation: Document requirement clearly. PipeWire is default on Ubuntu 22.04+, Fedora 34+, and Arch. Adoption is near-universal on modern Linux desktop.

7. **ChromaDB in sidecar complexity**: ChromaDB adds a persistent vector database to the sidecar, changing it from a thin ML worker to a stateful service. Mitigation: On-demand sidecar lifecycle with warm-up. ChromaDB persists to disk. The sidecar starts lazily and stays running while the app is open.

### Open Questions (to resolve during implementation)

- **Whisper model storage location**: App bundle vs. app data directory vs. user-configurable path? Bundled `small` model should be in the bundle; downloaded models should go to app data.
- **ffmpeg build configuration**: Which exact codecs and format support to include in the bundled ffmpeg? Minimal build (H.264/AAC/MP4 only) vs. broader format support for future use.
- **Meeting title auto-generation**: Should the app auto-generate a title from the first few seconds of transcript if the user didn't provide one? Nice UX touch, but depends on transcription completing first.

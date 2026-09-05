# Spec: Locus v1 — Offline-First Meeting Recording & Summarization

**Version:** 1.1 · **Reviewed:** 2026-09-05 · **Status:** approved for implementation, subject to documented feasibility gates

Read [PRD.md](./PRD.md) for product scope, [DESIGN.md](./DESIGN.md) for architecture, [CONTEXT.md](../../CONTEXT.md) for terminology, and [IMPLEMENTATION.md](./IMPLEMENTATION.md) for ordered tasks and owners. This document defines observable behavior and acceptance contracts.

---

## Problem Statement

Knowledge workers and students attend meetings, lectures, and discussions daily, but capturing actionable information from them is manual and error-prone. Existing solutions are either cloud-only (privacy concerns, internet dependency), proprietary (vendor lock-in), or only cover part of the workflow (transcription without summarization, or summarization without recording). There is no lightweight, cross-platform, offline-first tool that handles the full loop — recording a meeting, transcribing it with speaker identification, extracting presentation slides, and generating context-aware summaries with action items and deadlines — all locally on the user's machine.

## Solution

Locus is an offline-first desktop application (macOS + Windows + Linux) that records meetings (system audio, microphone, and/or screen), transcribes them with speaker diarization, extracts slide content from screen recordings, and generates AI-powered summaries tailored to the meeting type (business meeting vs. academic lecture). A built-in Knowledge Base allows users to upload supporting documents and semantically search across all meetings and documents via RAG-powered chat. Capture, transcription, diarization, and slides work on an offline first launch. Generation and semantic search work offline after skippable model provisioning, including local import. All processing runs locally by default using whisper.cpp for transcription, pyannote for speaker identification and voice activity detection, OpenCV for slide detection, ChromaDB for vector storage, and a bundled Llama.cpp server for summarization and embeddings. Ollama is an explicitly selected alternative; there is no automatic local-to-remote failover. The pipeline is designed for recordings up to 3 hours, using VAD-guided chunking for optimal performance. Users who prefer cloud AI can optionally configure API keys for OpenAI, Anthropic or Gemini. Linux support targets modern PipeWire-enabled distributions. The app stores everything in a local SQLite database (with ChromaDB for vector embeddings) and sends content off-device only for disclosed requests to an explicitly selected remote provider, including remote Ollama.

## User Stories

### Recording

1. As a knowledge worker, I want to start recording a meeting with one click, so that I can capture meeting content without switching between apps.

2. As a user, I want to choose which audio/video sources to capture (system audio, microphone, screen) via toggle switches before recording, so that I only capture what's relevant for this particular meeting.

3. As a user, I want to name my meeting before or during recording, so that I can find it easily later.

4. As a user, I want to see a live timer and audio level indicator during recording, so that I know the recording is working and capturing audio.

5. As a user, I want to pause and resume recording, so that I can skip breaks or irrelevant portions of a meeting.

6. As a user, I want continuous recoverable capture that finalizes into MP4 when I stop, so that an interruption preserves the recording and audio-only sessions remain playable.

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

26. As a user, I want a bundled llama-server engine with skippable local-model download/import setup, so that local summaries require no separately installed inference application.

27. As a user, I want to connect to an external Ollama instance or provide an API key for OpenAI, Anthropic or Gemini as an alternative, so that I can get higher-quality summaries if I prefer cloud AI.

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

38. As a user, I want the app to automatically check for updates and offer to install them, so that I stay on the latest version. I also want manual release downloads when online and installation of a previously downloaded verified package when offline.

### Knowledge Base

39. As a user, I want to upload supporting documents (PDF, Markdown, plain text) to the Knowledge Base, so that I can enrich the context available for AI-powered search and chat.

40. As a user, I want to semantically search across all my meetings and documents by meaning (not just keywords), so that I can find relevant discussions without remembering exact words.

41. As a user, I want to chat with the Knowledge Base ("What did we decide about the pricing model?"), so that I can get AI-powered answers grounded in my actual meeting transcripts and documents.

42. As a user, I want to ask questions about a specific meeting from the meeting detail view, so that I can quickly find information within that recording's context.

43. As a user, I want to link uploaded documents to specific meetings, so that the AI can use those documents as additional context when answering questions about that meeting.

44. As a user, I want each available source indexed automatically, independently of summary success, so that search readiness is seamless and failures remain discoverable.

45. As a user with a 3-hour recording, I want the pipeline to use voice activity detection to skip silence and chunk audio intelligently, so that transcription completes in reasonable time without degraded accuracy.

## Implementation Decisions

### Architecture

- **Desktop shell**: Tauri 2.11 with a React 19+ frontend rendered in the platform webview. Rust handles all backend orchestration, recording capture, transcription, database access, and process management.
- **Python sidecar**: A PyInstaller-packaged standalone binary.
  - **Sidecar responsibilities**: pyannote (VAD + diarization), OpenCV headless (slide detection), Tesseract (OCR), ChromaDB (vector storage for Knowledge Base).
  - **Sidecar lifecycle**: On-demand with warm-up. The sidecar starts lazily on first use (first pipeline run or first KB query) and stays running while the app is open. A ~3–5 second cold start is a benchmark target, not an established guarantee. Models are loaded on demand and may be unloaded under resource pressure. Validate cold-start targets on clean machines.
  - **OpenCV**: Uses `opencv-python-headless` to reduce sidecar size (no GUI dependencies).
- **Communication protocol**: JSON-RPC 2.0 over Tauri's sidecar stdin/stdout pipes. Rust sends typed requests (`diarize`, `detect_slides`, `ocr_slide`), Python responds with results. Python can emit progress notifications (JSON-RPC notifications without `id`).
- **Single monorepo** with clean module boundaries: `src-tauri/` (Rust), `src/` (React), `sidecar/` (Python). Each Rust module has a single responsibility: `capture/`, `transcription/`, `pipeline/`, `sidecar/`, `llm/`, `db/`, `commands/`. The `commands/` layer is thin and delegates to domain modules.

### Recording

- **Capture**: Native platform adapters behind one Rust contract: ScreenCaptureKit video/system audio plus a separate microphone path for macOS 13+; Windows Graphics Capture video plus WASAPI loopback/microphone; PipeWire audio and screen-cast portal on Linux.
- **Linux capture**: PipeWire screen cast portal for Wayland screen capture and PipeWire audio capture for system audio. PipeWire-only — no PulseAudio or X11 fallback. Minimum: Ubuntu 22.04+, Fedora 34+, Arch Linux.
- **Encoding**: Rust owns bundled FFmpeg and continuously writes independently recoverable encoded segments with a durable manifest. Stop finalizes H.264/AAC MP4; audio-only MP4 has no dummy video. The build includes required decoding, PCM/resampling/mixing and frame/image support; broader public media formats are outside v1. Exact distributable encoder paths and recovery bounds are feasibility gates.
- **Separate audio tracks**: Preserve microphone and system audio as separate source streams, not merely left/right stereo channels. Create a normalized mixed playback/transcription stream with common media timestamps while retaining source streams for diarization. External-player default playback must include both sources.
- **User-selected sources**: System Audio, Microphone, Screen; defaults remain System Audio + Screen. Require at least one audio source. Microphone-only, in-person, and audio-only sessions are supported; all-off and screen-only are invalid.

### Transcription

- **Engine**: whisper-rs Rust bindings (from codeberg.org/tazz4843/whisper-rs) for whisper.cpp. Transcription runs in the Rust process with zero Python involvement.
- **Voice Activity Detection**: pyannote VAD runs in the Python sidecar on the mixed audio track before transcription. Identifies speech regions and outputs time ranges. Silence is skipped during transcription.
- **Audio chunking**: VAD-guided variable chunks with a 5-minute maximum. Contiguous speech regions are grouped into chunks; if a speech region exceeds 5 minutes (e.g., continuous lecture), it is split at the lowest-energy point within the region. Whisper processes each chunk independently. Timestamps are aligned to the original recording timeline.
- **GPU candidates** require build and correctness qualification; runtime detection only selects packaged, verified paths:
  - macOS Apple Silicon → Metal; CoreML only with verified model assets/build support
  - macOS Intel + AMD GPU → evaluate native Metal and Vulkan/MoltenVK separately; CPU baseline always available
  - Windows/Linux NVIDIA → CUDA
  - Windows/Linux AMD → qualified ROCm/HIP or Vulkan paths, contingent on supported hardware/drivers
  - Fallback → CPU on all platforms
- **Model management**: Standard Whisper small (466 MiB) is read-only in the app bundle. Downloaded Whisper, generation and embedding models default to a configurable app-data models directory, independently of meeting storage. Expose curated compatible variants, integrity, disk/RAM estimates and download/import/cancel/retry/delete/select states.
- **Quantized models**: Offer verified standard and quantized variants (such as q5_0, q5_1, q8_0 where compatible). Standard small remains the default. Publish measured sizes and tested compatibility per catalog entry rather than promising every variant for every model.
- **Language**: Whisper auto-detects by default. A language dropdown in the recording setup allows manual override.
- **Timing**: Post-recording only. Transcription runs after recording stops. Three-hour validation with VAD-guided chunking is required before release; no performance testing has been completed in this planning-only repository.

### Diarization

- **Engine**: pyannote-audio running in the Python sidecar.
- **Model**: `pyannote/speaker-diarization-community-1` is bundled with the PyInstaller binary. No HuggingFace token required at runtime.
- **Integration**: Rust sends the audio file path to the sidecar via JSON-RPC. Python runs pyannote, returns speaker-labeled time segments. Rust merges these with whisper-rs transcript segments to produce speaker-attributed text.

### Slide Extraction

- **Detection**: OpenCV frame differencing in the Python sidecar. Runs as a post-recording batch job over the MP4 video. Detects slide transitions by comparing frame similarity, extracts unique slide frames as images.
- **OCR**: Tesseract (via `pytesseract`) extracts text from each slide image. Bundled with PyInstaller.
- **Storage**: Slide images saved to disk. Metadata (timestamp, image path, OCR text) stored in SQLite.

### Summarization

- **LLM providers**: Bundled llama-server is primary; users explicitly select Ollama, OpenAI, Anthropic, or Gemini as alternatives. Generation weights are downloaded/imported through skippable setup. Stored credentials never activate a destination, and local failures never automatically select a remote provider.
- **Prompt routing**: Keyword heuristics on the transcript text automatically detect meeting type ("lecture" keywords → academic prompt, "sprint/deadline" keywords → business prompt, uncertain → generic prompt). User overrides update the selected type; replacing an existing summary requires explicit regeneration with the corrected template and retains prior revisions.
- **Output**: Summary markdown, extracted action items (with optional deadlines and assignees), and key decisions. Stored in SQLite linked to the meeting.

### Knowledge Base

- **Vector store**: ChromaDB running in the Python sidecar. Persists embeddings and metadata to disk. The sidecar manages ChromaDB lifecycle.
- **Embedding model**: Local GGUF model via managed llama-server, provisioned by skippable download/import setup. Show size/progress and offline/unavailable states. Rust requests vectors and sends explicit embeddings to ChromaDB; the vector store must not implicitly download a model or call an external embedding service.
- **Embedding pipeline**: Durable background jobs index transcripts, available slide text and standalone documents independently; summaries are indexed when available. Source revision, chunking configuration and embedding model identity determine freshness. Persistent per-source readiness and actionable retry complement the sidebar indicator; success toasts are not required for every chunk/source.
- **Document upload**: Text-bearing PDF via Rust extraction, Markdown and plain text; initial limit 50 MiB/file and 500 PDF pages. Validate limits before expensive work, reject encrypted/image-only PDFs and invalid/empty text with actionable messages. Deduplicate identical bytes by content hash; return the existing document for new meeting links. Keep extracted PDF page locations and text offsets for citations.
- **Document-meeting linking**: Documents exist independently in the Knowledge Base. Users can optionally tag documents to one or more meetings, providing additional RAG context for meeting-scoped queries.
- **Knowledge Base view** (`/knowledge`): Chat-first interface inspired by LLM desktop apps. A secondary sidebar lists past conversation threads. The main area is a full chat interface with an LLM model selector in the chat input area. Semantic search and document upload are accessible from the sidebar. Scope toggles: "This meeting", "All meetings", "Documents only", "Everything".
- **Per-meeting Q&A**: The meeting detail view includes an "Ask about this meeting" input that performs scoped RAG retrieval using the meeting's transcript, summary, and linked documents.

### Pipeline

- **Pipeline steps**: Recoverable capture → finalize/encode; audio → VAD → transcribe → diarize; video → slide detection → OCR; usable transcript plus completed/failed/skipped optional work → initial summary. Independent indexing consumes each published source revision. Automatic title generation follows available whole-session context.
- **Pipeline parallelism**: Balanced is default and bounds heavy workers; Maximum permits parallel work within measured resource budgets. Capture has priority in every mode. Queue meetings durably and suspend/defer heavy inference/indexing during recording; a non-cooperative worker must yield by cancellation/restart from safe checkpoints rather than competing with capture.
- **Audio track routing**: VAD/transcription use mixed audio; diarization retains source provenance and diarizes microphone speech by default. Only an explicitly enabled “Only me on this microphone” setting permits User assignment. Do not merge identities across sources from coincident timing alone; overlap/echo and uncertain alignments remain labeled as uncertain.

### Database

- **Engine**: SQLite via `rusqlite` on a dedicated database worker. Single-file database in the app data directory. Vector embeddings stored in ChromaDB (separate from SQLite).
- **Schema**: DESIGN defines the relational entities and integrity rules for meetings, capture manifests, attempts, artifact/summary revisions, speakers, actions, sources/chunks, index jobs, threads/messages/citations, downloads/migrations and cleanup. SQLite is authoritative for ownership and visibility; vectors remain rebuildable derived state.
- **Credentials**: API keys stored in the OS keychain via the `keyring` Rust crate (macOS Keychain, Windows Credential Manager, Linux Secret Service). NOT in SQLite.

### Frontend

- **Layout**: Sidebar-driven layout inspired by LLM desktop apps (not a 1:1 copy of Jan). Persistent left sidebar with 5 items: Recording, Meetings, Knowledge Base, Model Manager, Settings. A downloads indicator in the sidebar footer shows background model download progress.
- **UI framework**: Astryx (Meta's design system, 150+ components) with TailwindCSS for style overrides. UI design work uses the impeccable skill.
- **State management**: TanStack Query for server state (async data from Tauri commands), Zustand for client state (recording UI, active tabs).
- **Routing**: TanStack Router with type-safe routes.
- **Video playback**: Gemini selects and pins Plyr or Video.js after a Tauri-webview media/keyboard accessibility spike. Use an audio-capable player when video is absent. Transcript sync via `onTimeUpdate` events bidirectionally linked to transcript line highlighting and click-to-seek.
- **Views (v1)**: Recording (`/record`), Meetings (`/`), Meeting Detail (`/meeting/:id`), Knowledge Base (`/knowledge`), Model Manager (`/models`), Settings (`/settings` — 3-column layout with category nav).
- **Export**: Clipboard copy, Markdown file, PDF, JSON — all triggered from the Meeting Detail view.

### Error Handling

- **Granular per-step pipeline status**: Persist pending, running, done, error, blocked, skipped and canceled with reason codes. Successful outputs have independent current/outdated freshness. Attempts are immutable identifiers with progress scoped to meeting + job + attempt, so delayed events cannot overwrite a retry.
- **Partial results preserved**: Keep last successful outputs visible during failures/retries. Optional step failure does not block initial summary from a usable transcript. Missing models produce an actionable blocked state; missing video/no slides/no speech yield explicit skipped/empty outcomes. Existing summary replacement is explicit.
- **Progress reporting**: Sidecar progress carries request/job/attempt identifiers; Rust validates and persists user-visible state. Frontend uses typed query snapshots and events or bounded polling, including resynchronization on reopen. Process crashes leave recoverable interrupted attempts, not permanently running rows.

### Distribution

- **License**: MIT for original Locus code. Preserve separate code/model notices, exact versions and applicable redistribution obligations in packaged artifacts.
- **Platforms**: macOS 13+ Apple Silicon/Intel (DMG + Homebrew; universal only if all native dependencies qualify), Windows x64 installers with qualified GPU variants as needed and CPU fallback, Linux AppImage + Flatpak on validated PipeWire/portal desktops. Exact native packaging must pass DESIGN's feasibility gates.
- **Auto-update**: One stable release channel, no maintained public beta channel. Check only when enabled/online, verify signed artifacts and install with user action while capture/data-changing jobs are idle. Package-manager installs follow their supported update mechanism; manual verified release installation remains available.
- **Bundled assets**: FFmpeg, standard Whisper small, verified pyannote dependencies/model assets, Tesseract executable/traineddata, Python sidecar with ChromaDB, llama-server. Provision generation and embedding weights separately. Validate runtime independence from Python installations, network access and developer caches.

## Behavioral Contracts

### Capture and recovery

- Rust owns capture state: idle → preparing → recording ↔ paused → finalizing → saved; failures enter interrupted/recoverable or an explicit unrecoverable error. UI navigation/window closure never owns capture lifetime. Start/stop/pause/resume commands are idempotent and reject incompatible transitions; only one active capture is allowed.
- Validate required permissions, selected source availability, writable storage and usable encoder before capture. Never silently downgrade selected capture sources. Reconcile all streams to a common monotonic media timeline; pause intervals are absent from output, and all transcript/slide/citation offsets use that timeline.
- Save a durable capture manifest and independently decodable segments continuously. On process restart, offer recovery and never restart capture automatically. Target at most five seconds of tail loss for process crashes on validated storage; do not equate this with a power-loss or failed-disk guarantee. Reserve finalization headroom and preserve completed segments when writes fail.
- Disk full, forced sleep, permission revocation and selected-source disappearance stop and preserve. Closing the window during capture keeps system controls visible; explicit quit offers Stop and save or Cancel. Lock alone need not stop if the OS continues valid selected-source capture. Three hours triggers a warning, not forced stop.
- If no video exists, skip slides/OCR with a reason. Empty VAD output is a successful no-speech result; skip speech-derived summaries and avoid hallucinated text. VAD failure falls back to bounded sequential chunks. Empty or failed transcription never silently becomes a slides-only meeting summary.
- Deduplicate speech at chunk boundaries, preserve gaps/offsets through VAD, and keep ambiguous/overlapping speaker attribution explicit. A source labeled User requires the saved single-person microphone setting.

### Processing, revision and user edits

- Snapshot model/provider/language/prompt settings into each job. An attempt publishes only when its source revision, meeting visibility and cancellation token still match. Retries are idempotent and never append duplicate transcript/slides/actions.
- Depend on terminal optional states, not only success: initial summary proceeds after diarization and applicable OCR finish/fail/skip. An error in an optional step is disclosed as missing input. Missing generation models block only generation; missing embedding models block only semantic readiness.
- Successful source replacement marks affected derived output outdated, queues current indexing and leaves previous output available. Replacement of an existing summary requires explicit regeneration; retain previous summary revisions and their completed action items. New revision items do not steal or reset old completion records; transfer completion only with an explicit, evidenced identity mapping or user action.
- Speaker names are stable references rendered into structured generated content; rename updates these references without global free-text replacement. Original quoted evidence remains unchanged. Unknown people and ambiguous relative deadlines remain unknown/quoted rather than fabricated; retain source evidence and meeting timezone for any normalization.
- For three-hour input, budget tokens across hierarchical summary passes with provenance covering the full session. Never silently truncate to the first context window. Validate structured model outputs and citations before publication; invalid output leaves previous results available and exposes retry.

### Provider selection and grounded chat

- Saving keys or discovering Ollama makes providers available; explicit selection authorizes generation at that destination. Before selection, disclose that summaries/titles use meeting text and that chat may send the question, retained conversation context and retrieved source excerpts. Non-loopback Ollama is remote. Do not silently change provider after failure.
- Keep managed inference servers loopback-only with per-launch authentication and ownership checks. Rust owns provider credentials and requests; frontend receives redacted configuration only. Missing/locked keychains produce actionable errors and no plaintext fallback. Disable dependency telemetry and redact content/secrets from logs.
- Persist threads locally with a fixed scope. Changing scope creates a new thread and leaves the old one intact. This meeting means its transcript, applicable slide text, current summary and linked documents; require an actual meeting selection. All meetings excludes independent documents; Documents only excludes meeting results; Everything includes both. Provider/model may change per message with visible destination and provenance.
- Enforce source ownership, scope and visibility before retrieval; exclude deleted/outdated sources from new answers. Treat documents, transcripts and model text as untrusted content, never as instructions to execute commands, expose secrets or fetch remote resources.
- Answers and generated factual claims carry validated citations to transcript times, slide times, PDF pages or text locations. Insufficient sources produce an explicit inability to answer; do not invent references, assignees or deadlines. A scope change never inherits messages from the previous scope. Streaming cancel/error preserves an explicitly incomplete message and does not imply completion.

### Knowledge Base ownership, indexing and deletion

- SQLite owns source identity, revisions and visibility; ChromaDB is rebuildable derived storage. Persist independent indexing jobs for each source with blocked/running/ready/error states, cancellation and durable retry. Index each current source once regardless of the number of meeting links.
- Store embedding model ID/revision, vector dimension, chunker version and source revision with each index generation. An embedding-model change builds a separate generation; never mix vectors from different models. Keep an old compatible generation searchable when its model remains available until replacement is ready, otherwise display unavailable/rebuilding status. Prevent deleting models currently in use.
- File validation rejects the entire unsupported upload rather than silently publishing incomplete content. Scanned/image-only and encrypted PDFs need an actionable explanation; mixed PDFs index available text and disclose omitted images. Deduplicate exact bytes, retaining a single independently owned document and optional many-to-many links.
- Delete starts with a visibility tombstone and job fence in SQLite, then durable cleanup of owned media, intermediate artifacts, vectors and chat data. Query/export paths honor tombstones immediately. Restart retries cleanup; late sidecar/provider responses cannot republish deleted content.
- Meeting deletion removes its scoped threads and links, not independent documents. Document deletion removes its links and chunks. In mixed-source threads, remove source-dependent messages and their downstream dependent turns/context to prevent retention through later answers; remove associated citations/caches too. Invalidate active responses using a deleted source. Explain that user exports and already sent remote requests cannot be recalled by local deletion.
- Meeting data relocation coordinates SQLite, ChromaDB and owned media while capture and data-changing jobs are idle: verify a complete copy before switching, preserve the original on failure and recover an interrupted migration. Model-directory migration is independent. Use app-owned identifiers/relative paths; never treat an arbitrary configured directory as wholly owned or delete unrelated files.

### Contributor and release acceptance

- Every behavior change cites its PRD requirement and SPEC contract, with meaningful tests of normal and relevant failure paths. Bug fixes include a reproducing regression where automation is feasible. Test public results/contracts, not mocks that merely echo the implementation.
- Fast CI must pass on the PR revision: formatting/linting, type checks, builds, CPU backend tests, sidecar protocol tests and critical frontend interactions. Contract/schema changes require compatibility/migration tests; concurrency, deletion, retries, provider authorization and recovery require targeted negative/race cases. A documentation-only change uses link/consistency validation and does not require inference tests.
- Never remove/weaken assertions, hide failures, add blanket skips or rewrite product requirements just to make a change pass. Explain pre-existing failures separately and require maintainer disposition for exceptions; hardware limitations are reported as unverified, not passing.
- Maintain one stable channel. Develop feature-complete macOS then Linux and Windows, using ordinary development builds for testing. v1.0 requires clean-install, upgrade, capture, playback, bundled-offline and hardware/backend evidence on all supported platforms. Manual checks supplement automated tests; they do not replace automatable contracts.
- The implementation plan includes CI workflows, a PR evidence template, CODEOWNERS/review guidance and maintainer-enabled required status checks/branch protection. AGENTS.md is contributor guidance, not an enforcement mechanism by itself. Publishing, merging and remote repository settings require maintainer authority.

## Testing Decisions

### What makes a good test

A good test for Locus exercises **external behavior through the defined seams**, not internal implementation details. Tests should verify that given a known input (an audio/video file), the system produces the expected output (transcript segments with speaker labels, extracted slides, a summary). Tests should not assert on private data structures or internal call order. Persisted lifecycle states and progress exposed through public contracts are observable behavior and should be tested. If you can refactor the internals and the tests still pass, the tests are good.

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

### Hardware and UI validation

- **Recording capture**: ScreenCaptureKit / Windows Graphics Capture require actual hardware, OS permissions, and active audio sources. These are tested manually per platform during release QA.
- **GPU backend selection**: Runtime detection of Metal/CUDA/ROCm/Vulkan depends on hardware. CI runs CPU-only. GPU backends are tested manually on target hardware.
- **Frontend UI**: v1 includes critical interaction/contract tests: capture controls, close/reopen state, recovery and retry, provider selection, fixed-scope chat, indexing readiness, deletion, titles, and keyboard navigation. Gemini 3.8 Flash High owns frontend implementation and tests using impeccable. Broad component snapshots/visual regression coverage may wait.

### Test tooling

- **Rust**: `cargo test` with `#[tokio::test]` for async pipeline tests
- **Python**: `pytest` for sidecar RPC handler tests
- **Fixtures**: Committed to `tests/fixtures/` — small, deterministic audio/video files
- **Frontend**: TypeScript checking, linting, production build, Vitest/Testing Library critical-flow tests and bounded browser/webview smoke tests
- **CI**: GitHub Actions — backend formatting/linting, `cargo test` (CPU baseline), `pytest`, frontend static/build/critical-flow checks, and document/contract checks on pull requests and pushes. Fast tests are offline and deterministic; actual bundled engines and hardware are validated separately.

## Out of Scope

The following are explicitly **not** in v1 and are deferred to future versions:

- **Real-time transcription** (v2): Live transcription during recording. v1 is post-recording only.
- **Dashboard & Calendar views** (v2): Overview stats, deadline tracking, timeline view of meetings.
- **Speaker voice profiles** (v3): Auto-identifying returning speakers across meetings by voice fingerprint.
- **Custom prompt template editor** (v3): User-editable summarization prompt templates.
- **Mobile companion app** (v3): Mobile viewing of summaries and transcripts.
- **Meeting scheduling integration** (future): Calendar integration (Google Calendar, Outlook) for auto-naming or auto-triggering recordings.
- **Collaboration / team features** (future): Sharing meetings, team workspaces, shared knowledge bases.
- **Broad frontend visual regression coverage**: Deferred until the design language stabilizes; critical interaction tests are required in v1.
- **Image context** (v2): Standalone images and document-embedded images, OCR plus visual understanding of charts, diagrams and screenshots, with image/page citations. Formats, models and resource limits require a dedicated v2 spec; v1 document ingestion remains text-based.

## Resolved Implementation Questions

1. **Model storage**: Bundled standard Whisper small remains read-only inside the bundle. Managed downloads default to app data with a separate configurable models folder. Migration runs only when affected models/downloads are idle: preflight capacity/access, copy and verify, atomically switch metadata, then offer cleanup of the old app-owned copies. On failure preserve originals and the previous configuration. Imported source files remain user-owned; import copies verified compatible weights into managed storage. Missing removable storage marks affected models unavailable without silently duplicating downloads or disabling bundled capture/transcription.
2. **Media format scope**: H.264/AAC MP4 is the v1 saved-media format, with audio-only MP4 allowed. Internal raw/PCM decoding, mixing/resampling and frame/image processing remain required. Pin and validate exact encoder/build recipes and notices before distribution; do not assume hardware encoding exists on every supported machine.
3. **Automatic titles**: Persist a date/time placeholder at capture start with title origin and revision. After transcription, prefer the complete meeting summary as title context; otherwise use bounded passages spanning the transcript if a selected generation provider is available. Never require a minimum five-minute meeting or privilege only the first 30 minutes. Commit an automatic title only if the placeholder's expected revision still matches. Explicit user edits always win; no provider leaves the placeholder. Regeneration does not automatically replace a non-placeholder title.

## Engineering Feasibility Gates

These require measurements or runnable proof, not another product interview. Record exact versions, artifacts and results before dependent implementation proceeds; any necessary product change returns to the maintainer with evidence.

- Clean-machine packaging: compatible Python/PyTorch/pyannote/ChromaDB/PyInstaller versions, model redistribution, no runtime token/network requirement for bundled features, Tesseract binaries and language data. Do not hard-pin Python 3.14 without proof that the complete dependency set works.
- Platform media: macOS 13 system audio plus microphone path; Windows Graphics Capture + WASAPI; Linux PipeWire audio and working screen portal. Distribution names alone do not establish capability.
- FFmpeg: exact minimal build, H.264/AAC compatibility, usable hardware and distributable fallback encoders, recovery segments and ≤5-second crash-loss target under declared disk/fsync conditions.
- GPU: CPU correctness baseline and actual packaged backend initialization/fallback; Intel/AMD llama.cpp PR #19527 concerns Metal and is distinct from MoltenVK. Pin a fork/commit only if a measured spike establishes its need. Prebuilt artifacts require checksums and reproducible build recipes.
- Resource budgets: 8GB capture priority, model load/unload, three-hour mixed and continuous speech, VAD boundaries/overlap, hierarchical generation without truncation, and actual installer/installed sizes. Performance targets are unverified until measured on named hardware.
- Shell integration: persistent close-to-background controls on each supported desktop. If a Linux tray is unavailable, retain an accessible recording-control window; do not hide a recording without a discoverable control surface.
- Tauri media, signed updates and package-manager channels: native-webview playback/seek support and actual install/upgrade workflows per artifact; no update during capture or storage migration.

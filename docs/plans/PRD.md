# Locus — Product Requirements Document (PRD)

> **Source of Truth** — This document defines *what* Locus does and *why*. For *how* it's built, see [DESIGN.md](file:///Volumes/Storage/DevShit/Locus/docs/plans/DESIGN.md). For engineering-level user stories and testing, see [SPEC.md](file:///Volumes/Storage/DevShit/Locus/docs/plans/SPEC.md).

**Version**: 1.0
**Last Updated**: 2026-09-01
**Status**: Approved
**License**: Open Source (MIT or Apache 2.0)

---

## 1. Product Vision

**Locus is the open-source, offline-first meeting recorder that turns conversations into structured knowledge — without your data ever leaving your machine.**

Every meeting produces information that decays: action items are forgotten, deadlines are missed, decisions are relitigated. Cloud recording tools exist, but they require internet, send your data to third-party servers, and lock you into proprietary ecosystems. Locus eliminates all three problems by running the entire pipeline — recording, transcription, speaker identification, slide extraction, and AI summarization — locally on the user's device.

### Product Positioning

| | Cloud Tools (Otter, Fireflies, etc.) | Locus |
|---|---|---|
| Data privacy | ❌ Data sent to cloud servers | ✅ Everything stays local |
| Internet required | ❌ Always | ✅ Never (unless user opts into cloud AI) |
| Vendor lock-in | ❌ Proprietary formats | ✅ Open source, standard formats (MP4, Markdown, JSON) |
| Cost | ❌ Monthly subscription | ✅ Free forever |
| Slide extraction | ❌ Rarely supported | ✅ Built-in (OpenCV + OCR) |
| Speaker ID | ⚠️ Often cloud-only | ✅ Local (pyannote) |
| Cross-platform | ⚠️ Varies | ✅ macOS + Windows + Linux (v1) |

---

## 2. Target Personas

### Persona 1: Knowledge Worker ("Morgan")

- **Role**: Product manager, engineer, designer, or team lead
- **Context**: Attends 4–8 meetings/week via Zoom, Google Meet, or Teams
- **Pain**: After back-to-back meetings, action items and decisions blur together. Manually writing notes during meetings is distracting.
- **Jobs to be done**:
  - Record a meeting without thinking about it
  - Get a list of action items + deadlines after the meeting
  - Find "what did we decide about X?" weeks later
  - Share meeting summaries with absent teammates
- **Success looks like**: After every meeting, Morgan has a clean summary with checkable action items within minutes, without having taken a single note.

### Persona 2: Student ("Alex")

- **Role**: University or online course student
- **Context**: Attends 2–4 lectures/week, some with slides. May be in a different timezone (recorded lectures).
- **Pain**: Keeping up with lecture notes while listening is hard. Misses nuances. Slides are shared inconsistently.
- **Jobs to be done**:
  - Record a lecture and get searchable notes afterward
  - Review specific moments by searching the transcript
  - Get slide content even when the professor doesn't share them
  - Study from auto-generated key concept summaries
- **Success looks like**: Alex records a 90-minute lecture and gets organized study notes with key concepts, extracted slides, and timestamps to revisit confusing sections.

### Shared Needs

Both personas need:
- Zero-friction recording (≤ 2 clicks to start)
- No cloud dependency (works on a plane, in a dorm, on restricted networks)
- Low resource usage (recording shouldn't slow down their machine)
- Private by default (no accounts, no telemetry, no data upload)

---

## 3. Product Principles

1. **Local by default, cloud by choice** — Every feature works offline. Cloud APIs are an opt-in upgrade, never a requirement.
2. **Capture everything, organize later** — The recording pipeline should be reliable and fire-and-forget. Organization (summaries, action items) happens automatically after the meeting.
3. **Partial results are better than no results** — If one pipeline step fails (e.g., diarization crashes), everything else that succeeded is still available. Never lose data.
4. **The user controls their data** — Standard formats (MP4, SQLite, Markdown). No proprietary lock-in. Export everything.
5. **Smart defaults, full control** — Auto-detect language, auto-detect meeting type, auto-select GPU backend. But always let the user override.

---

## 4. Functional Requirements

### FR1: Meeting Recording

| ID | Requirement | Priority |
|----|------------|----------|
| FR1.1 | The app shall capture **system audio** from the operating system's audio output (to record meeting participants in a video call) | P0 |
| FR1.2 | The app shall capture **microphone audio** from the user's input device | P0 |
| FR1.3 | The app shall capture **screen video** (full screen or specific window) | P0 |
| FR1.4 | The user shall be able to independently enable/disable each capture source (system audio, microphone, screen) via toggle switches before recording | P0 |
| FR1.5 | Default capture sources shall be System Audio (on) + Screen (on) + Microphone (off) | P1 |
| FR1.6 | The user shall be able to enter a meeting title before or during recording | P0 |
| FR1.7 | The user shall be able to select a meeting type (Auto-detect, Meeting, Lecture) before recording | P1 |
| FR1.8 | The app shall display a live timer showing elapsed recording time | P0 |
| FR1.9 | The app shall display a live audio level indicator during recording | P1 |
| FR1.10 | The user shall be able to pause and resume recording | P1 |
| FR1.11 | On stopping the recording, the app shall automatically encode the captured streams into an MP4 file (H.264 video + AAC audio) using the bundled ffmpeg | P0 |
| FR1.12 | Recording capture shall use native platform APIs: ScreenCaptureKit (macOS), Windows Graphics Capture (Windows), PipeWire (Linux) | P0 |
| FR1.13 | The app shall capture system audio and microphone audio as **separate audio tracks** (not mixed), preserving per-source audio for downstream processing | P0 |
| FR1.14 | On Linux, recording capture shall use PipeWire screen cast portal (Wayland) and PipeWire audio capture. Minimum requirement: PipeWire-enabled system (Ubuntu 22.04+, Fedora 34+, Arch Linux) | P0 |

### FR2: Transcription

| ID | Requirement | Priority |
|----|------------|----------|
| FR2.1 | After recording completes, the app shall automatically transcribe the audio using whisper-rs (whisper.cpp Rust bindings) | P0 |
| FR2.2 | Transcription shall produce timestamped text segments with start and end times | P0 |
| FR2.3 | The app shall auto-detect the spoken language using Whisper's built-in language detection | P0 |
| FR2.4 | The user shall be able to manually specify the language before recording via a dropdown | P1 |
| FR2.5 | The app shall automatically select the best available GPU backend at runtime: Metal/CoreML (Apple Silicon), Vulkan/MoltenVK (Intel Mac + AMD GPU), CUDA (NVIDIA on Windows/Linux), ROCm/HIP (AMD on Windows/Linux), CPU (fallback) | P0 |
| FR2.6 | The app shall bundle the Whisper `small` model (~250MB) for immediate out-of-box use | P0 |
| FR2.7 | The app shall provide an in-app model manager where users can download additional models (`tiny`, `base`, `medium`, `large-v3`) | P1 |
| FR2.8 | The model manager shall display estimated RAM/VRAM requirements per model | P2 |
| FR2.9 | Transcription shall run post-recording only (not real-time), using VAD-guided chunking for optimal performance on long recordings (up to 3 hours) | P0 |
| FR2.10 | The app shall run Voice Activity Detection (VAD) via pyannote before transcription to identify speech regions and skip silence | P0 |
| FR2.11 | The app shall chunk audio into VAD-guided segments with a maximum chunk length of 5 minutes, splitting at low-energy points when a speech region exceeds the maximum | P0 |
| FR2.12 | The model manager shall offer both standard and quantized (q5_0, q5_1, q8_0) variants of each Whisper model, showing size and quality tradeoffs | P1 |

### FR3: Speaker Diarization

| ID | Requirement | Priority |
|----|------------|----------|
| FR3.1 | After transcription, the app shall run speaker diarization using pyannote-audio to assign speaker labels to transcript segments | P0 |
| FR3.2 | The pyannote `speaker-diarization-community-1` model shall be bundled with the app (no HuggingFace token required) | P0 |
| FR3.3 | Each identified speaker shall be assigned a distinct color for display in the UI | P1 |
| FR3.4 | The user shall be able to rename speaker labels (e.g., "Speaker 1" → "Alice") | P1 |
| FR3.5 | If diarization fails, the undiarized transcript shall remain available | P0 |

### FR4: Slide Extraction

| ID | Requirement | Priority |
|----|------------|----------|
| FR4.1 | For recordings that include screen capture, the app shall detect slide transitions using OpenCV frame differencing | P1 |
| FR4.2 | The app shall extract each unique slide as a separate image file | P1 |
| FR4.3 | Each extracted slide shall be linked to its timestamp in the recording | P1 |
| FR4.4 | The app shall extract text content from slide images using Tesseract OCR | P1 |
| FR4.5 | Extracted slide text shall be included in the context sent to the LLM for summarization | P1 |
| FR4.6 | Slide extraction shall run as a post-recording batch process (no impact on recording performance) | P0 |

### FR5: AI Summarization

| ID | Requirement | Priority |
|----|------------|----------|
| FR5.1 | After transcription (and optionally diarization + slide extraction), the app shall generate an AI-powered summary | P0 |
| FR5.2 | The app shall support **Ollama** as a local LLM provider (auto-detected at `localhost:11434`) | P0 |
| FR5.3 | The app shall support **cloud API providers** (OpenAI, Anthropic) via user-provided API keys | P0 |
| FR5.4 | The app shall use **keyword heuristics** on the transcript to auto-detect meeting type and select the appropriate summarization prompt | P1 |
| FR5.5 | For **business meetings**, the summary shall include: overview, action items with deadlines and assignees, key decisions | P0 |
| FR5.6 | For **lectures**, the summary shall include: overview, key concepts, assignments/homework, important dates | P0 |
| FR5.7 | The user shall be able to override the auto-detected meeting type and re-run summarization with the corrected prompt | P1 |
| FR5.8 | Action items shall be individually checkable (mark as completed) | P1 |
| FR5.9 | If the LLM is unavailable, the transcript and slides shall remain viewable without a summary | P0 |

### FR6: Meeting Management

| ID | Requirement | Priority |
|----|------------|----------|
| FR6.1 | The app shall display a list of all past recordings showing: title, date/time, duration, number of speakers, meeting type badge, processing status | P0 |
| FR6.2 | The user shall be able to search meetings by title | P0 |
| FR6.3 | The user shall be able to filter meetings by type (Meeting, Lecture, All) and date range | P1 |
| FR6.4 | The user shall be able to delete meetings (including all associated files and data) | P0 |
| FR6.5 | Each pipeline processing step shall display its own status (pending, running, done, error) independently | P0 |
| FR6.6 | The user shall be able to retry any individual failed pipeline step | P0 |

### FR7: Meeting Detail & Playback

| ID | Requirement | Priority |
|----|------------|----------|
| FR7.1 | The meeting detail view shall display video playback (Plyr or Video.js) with standard controls: play/pause, seek bar, volume, playback speed, fullscreen | P0 |
| FR7.2 | The meeting detail view shall display the transcript as a scrollable list of timestamped, speaker-labeled segments | P0 |
| FR7.3 | Clicking a transcript line shall seek the video player to that line's timestamp | P0 |
| FR7.4 | As the video plays, the currently active transcript line shall be visually highlighted | P0 |
| FR7.5 | The meeting detail view shall include a **Summary tab** showing the AI-generated summary and action items | P0 |
| FR7.6 | The meeting detail view shall include a **Slides tab** showing a gallery of extracted slide images with timestamps | P1 |
| FR7.7 | Clicking a slide in the gallery shall seek the video player to that slide's timestamp | P1 |

### FR8: Export

| ID | Requirement | Priority |
|----|------------|----------|
| FR8.1 | The user shall be able to **copy the summary to the clipboard** with one click | P0 |
| FR8.2 | The user shall be able to export the meeting as a **Markdown** file (transcript + summary) | P1 |
| FR8.3 | The user shall be able to export the meeting as a **PDF** document | P2 |
| FR8.4 | The user shall be able to export the meeting as a **JSON** file (structured data for programmatic access) | P2 |

### FR9: Settings & Configuration

| ID | Requirement | Priority |
|----|------------|----------|
| FR9.1 | The user shall be able to select the active Whisper model and download additional models | P0 |
| FR9.2 | The user shall be able to configure the LLM provider (Ollama URL, OpenAI API key, Anthropic API key, Gemini API key) | P0 |
| FR9.3 | API keys shall be stored in the **OS keychain** (macOS Keychain, Windows Credential Manager) — not in plaintext | P0 |
| FR9.4 | The user shall be able to set default recording capture sources | P2 |
| FR9.5 | The user shall be able to configure the storage location for recordings and data | P2 |
| FR9.6 | The user shall be able to toggle between dark and light themes | P2 |
| FR9.7 | The app shall display current storage usage | P2 |

### FR10: Knowledge Base

| ID | Requirement | Priority |
|----|------------|----------|
| FR10.1 | The app shall provide a Knowledge Base view (`/knowledge`) combining a document/meeting list with semantic search and a slide-in chat panel | P0 |
| FR10.2 | The user shall be able to upload supporting documents (PDF text extraction, Markdown, plain text) to the Knowledge Base | P0 |
| FR10.3 | Uploaded documents shall be optionally linkable to one or more meetings | P1 |
| FR10.4 | The app shall generate vector embeddings for transcript chunks, summaries, and uploaded documents using EmbeddingGemma-300M (bundled, via Rust ONNX Runtime) | P0 |
| FR10.5 | Embeddings shall be stored in ChromaDB (running in the Python sidecar) for similarity search | P0 |
| FR10.6 | The Knowledge Base shall support semantic search across all meetings and documents ("find meetings where we discussed the pricing model") | P0 |
| FR10.7 | The Knowledge Base shall include a chat interface where users can ask questions answered via RAG (retrieve relevant chunks → send to LLM with question) | P0 |
| FR10.8 | The meeting detail view shall include an "Ask about this meeting" input for scoped Q&A using the meeting's transcript and linked documents | P1 |
| FR10.9 | Embedding generation shall run as a silent background process after summarization, with toast notifications for status and errors | P1 |
| FR10.10 | The user shall be able to scope chat queries: "This meeting only", "All meetings", "Documents only", or "Everything" | P1 |

### FR11: Updates

| ID | Requirement | Priority |
|----|------------|----------|
| FR11.1 | The app shall check for updates via Tauri's built-in updater when internet is available | P1 |
| FR11.2 | Updates shall also be downloadable manually from GitHub Releases | P0 |
| FR11.3 | The app shall never block or require an update to function (offline-first) | P0 |

---

## 5. Non-Functional Requirements

### Performance

| ID | Requirement | Target |
|----|------------|--------|
| NFR1 | Recording shall not cause dropped frames or audio glitches on a machine with ≥ 8GB RAM | 0 dropped frames per 60-minute recording |
| NFR2 | Transcription of a 60-minute recording shall complete within a reasonable time using the `small` model | ≤ 15 minutes on Apple M1, ≤ 25 minutes on CPU |
| NFR2a | Transcription of a 3-hour recording shall complete using VAD-guided chunking | ≤ 45 minutes on Apple M1 (GPU), ≤ 75 minutes on CPU |
| NFR2b | VAD pre-filtering shall reduce whisper processing time by ≥ 30% on recordings with significant silence | - |
| NFR3 | App cold start (launch to ready-to-record) | ≤ 5 seconds |
| NFR4 | Meeting list load time (100 meetings) | ≤ 1 second |
| NFR5 | Video seek + transcript sync response time | ≤ 200ms |

### Security & Privacy

| ID | Requirement |
|----|------------|
| NFR6 | No user data shall be transmitted over the network unless the user explicitly configures a cloud LLM provider |
| NFR7 | No telemetry, analytics, or crash reporting shall be collected without explicit opt-in |
| NFR8 | API keys shall be stored using OS-level secure credential storage (keychain) |
| NFR9 | All meeting data (recordings, transcripts, summaries) shall be stored locally in the user's app data directory |

### Compatibility

| ID | Requirement |
|----|------------|
| NFR10 | macOS: Support macOS 12.3+ (Monterey — required for ScreenCaptureKit) on both Apple Silicon and Intel |
| NFR11 | Windows: Support Windows 10 1903+ (required for Windows Graphics Capture) on x64 |
| NFR12 | Recordings shall be saved as standard MP4 (H.264 + AAC) playable in any media player |
| NFR12a | Linux: Support Ubuntu 22.04+, Fedora 34+, and Arch Linux with PipeWire as the audio/screen capture backend. PipeWire is required; PulseAudio-only systems are not supported. |

### Install Size

| ID | Requirement | Target |
|----|------------|--------|
| NFR13 | Base app (Tauri + React + whisper-rs + ffmpeg + `small` model + EmbeddingGemma-300M) | ≤ 500MB |
| NFR14 | Full app (base + Python sidecar with pyannote + OpenCV + Tesseract + ChromaDB) | ≤ 4.5GB |

### Accessibility

| ID | Requirement |
|----|------------|
| NFR16 | All interactive elements shall be keyboard-navigable |
| NFR17 | All UI components shall use Astryx's built-in ARIA attributes for screen reader support |
| NFR18 | Color is never the sole indicator of information (e.g., speaker colors are supplemented with labels) |

---

## 6. System Architecture Summary

> Full architecture details are in [DESIGN.md](file:///Volumes/Storage/DevShit/Locus/docs/plans/DESIGN.md).

```mermaid
graph TB
    subgraph "User's Machine"
        subgraph "Tauri App"
            FE["React Frontend<br/>(Astryx + TanStack)"]
            BE["Rust Backend<br/>(whisper-rs, capture, orchestrator)"]
            DB[(SQLite)]
        end
        subgraph "Sidecar"
            PY["Python Binary<br/>(pyannote, OpenCV, Tesseract)"]
        end
        subgraph "External (Optional)"
            OL[Ollama]
        end
    end
    subgraph "Cloud (Opt-in Only)"
        CL["OpenAI / Anthropic / Gemini"]
    end

    FE <-->|Tauri Commands| BE
    BE <-->|JSON-RPC 2.0| PY
    BE <--> DB
    BE -.->|if configured| OL
    BE -.->|if configured| CL
```

**Key architectural decisions:**
- **Rust orchestrates everything** — recording, transcription (whisper-rs), pipeline management, database, IPC
- **Python sidecar is thin** — only ML workloads (pyannote, OpenCV, Tesseract), no orchestration
- **JSON-RPC 2.0 over stdin/stdout** — structured communication between Rust and Python
- **SQLite** — single-file database, zero configuration, offline-first

---

## 7. Acceptance Criteria

### Recording

- [ ] User can start a recording with ≤ 2 interactions (open record view → press record)
- [ ] System audio, microphone, and screen can be independently toggled
- [ ] Recording produces a valid MP4 file playable in the app's video player
- [ ] Pausing and resuming produces a continuous final recording without gaps or artifacts
- [ ] Recording timer accurately reflects elapsed time

### Transcription

- [ ] A 5-minute English audio file produces transcript segments that match the spoken content (≥ 80% word accuracy with `small` model)
- [ ] Segments include accurate start/end timestamps (within ±1 second)
- [ ] Language is correctly auto-detected for English, Spanish, Mandarin, Japanese, and French test fixtures
- [ ] GPU acceleration is used when available (verified by checking inference time vs. CPU baseline)
- [ ] CPU fallback works when no GPU is available

### Diarization

- [ ] A 2-speaker audio file produces segments attributed to 2 distinct speaker labels
- [ ] Speaker labels are consistent (the same voice always gets the same label within a meeting)
- [ ] Speaker colors are visually distinct in the transcript view
- [ ] Renaming a speaker updates all transcript lines, the summary, and action item assignees
- [ ] If diarization fails, the transcript is displayed without speaker labels (graceful degradation)

### Slide Extraction

- [ ] A screen recording with 5 distinct slides produces 5 extracted slide images
- [ ] Each slide's timestamp matches the actual transition point in the video (within ±2 seconds)
- [ ] OCR correctly extracts the primary text content from clean (non-handwritten) slides
- [ ] Clicking a slide in the gallery seeks the video to the correct timestamp

### Summarization

- [ ] Given a transcript of a business meeting discussing action items, the summary contains extracted action items with deadlines
- [ ] Given a transcript of a lecture, the summary contains key concepts and assignments
- [ ] Auto-detection correctly classifies a business meeting transcript (containing "deadline", "action item", "stakeholder" keywords)
- [ ] Auto-detection correctly classifies a lecture transcript (containing "assignment", "exam", "chapter" keywords)
- [ ] Overriding the meeting type and re-summarizing produces a different summary format
- [ ] If Ollama is not running and no API key is configured, the app shows a clear message and the transcript/slides remain viewable

### Pipeline Resilience

- [ ] Each pipeline step shows its independent status (pending/running/done/error) in the UI
- [ ] A failed diarization step does not prevent the transcript from being viewed
- [ ] A failed summarization step does not prevent transcript or slides from being viewed
- [ ] Retrying a failed step re-runs only that step, not the entire pipeline
- [ ] Progress percentage updates are reflected in the UI during long-running steps

### Export

- [ ] Clipboard copy produces a formatted summary string that pastes correctly into a text editor
- [ ] Markdown export produces a valid `.md` file containing the full transcript and summary
- [ ] JSON export produces valid JSON with structured meeting data

### Knowledge Base

- [ ] User can upload a PDF document and it appears in the Knowledge Base view
- [ ] Semantic search for a phrase discussed in a past meeting returns that meeting
- [ ] Chat query "What did we decide about X?" retrieves relevant transcript chunks and produces an LLM answer
- [ ] Documents can be linked to specific meetings and appear in the meeting detail view
- [ ] Embedding generation completes silently after summarization with a toast notification

### Linux Recording

- [ ] On Ubuntu 22.04+ with PipeWire, user can record system audio + screen via PipeWire portal
- [ ] Recording produces a valid MP4 file identical in format to macOS/Windows recordings
- [ ] PipeWire permission dialog appears on first recording attempt

### Long Recordings (3 hours)

- [ ] A 3-hour recording with mixed speech and silence completes the full pipeline without errors
- [ ] VAD correctly identifies speech regions and skips silence (≥ 30% time reduction vs. raw processing)
- [ ] Chunked transcription produces continuous, coherent transcript without missing words at chunk boundaries
- [ ] Memory usage during transcription stays bounded (no OOM on 8GB RAM machine with `small` model)

---

## 8. Success Metrics

### Adoption (post-launch)

| Metric | Target (6 months) |
|--------|--------------------|
| GitHub stars | 500+ |
| Monthly active installs | 1,000+ |
| Community contributors | 10+ |

### Quality

| Metric | Target |
|--------|--------|
| Transcription word accuracy (English, `small` model) | ≥ 80% |
| Diarization speaker error rate (2-speaker meetings) | ≤ 15% |
| Pipeline completion rate (all steps succeed) | ≥ 90% |
| App crash rate | < 1% of sessions |

### User Experience

| Metric | Target |
|--------|--------|
| Time from "stop recording" to "summary available" (30-min meeting, `small` model, GPU) | ≤ 5 minutes |
| Clicks to start recording | ≤ 2 |
| Clicks to copy summary to clipboard | 1 |

---

## 9. Release Plan

### Phase 1: Core Loop (v1.0)

**Goal**: Record → Transcribe → Diarize → Summarize → Knowledge Base → View (all platforms)

| Milestone | Scope |
|-----------|-------|
| **M1: Skeleton** | Tauri + React shell, SQLite schema, Python sidecar scaffolding, monorepo CI |
| **M2: Recording** | macOS ScreenCaptureKit, separate audio tracks (mic + system), ffmpeg encoding |
| **M3: Transcription** | whisper-rs integration, pyannote VAD, VAD-guided chunking (5-min max), GPU backend detection, model manager (standard + quantized variants) |
| **M4: Diarization + Slides** | pyannote sidecar (separate track routing: mic="User"), OpenCV headless slide detection, Tesseract OCR |
| **M5: Summarization** | Ollama + cloud API integration, prompt routing, action item extraction |
| **M6: Meeting Detail** | Video playback, transcript sync, summary view, slides gallery |
| **M7: Knowledge Base** | ChromaDB integration, EmbeddingGemma-300M (ONNX), document upload (PDF/MD/TXT), semantic search, chat panel, document-meeting linking |
| **M8: Polish + Export** | Search/filter, export (clipboard/MD/PDF/JSON), settings, auto-update |
| **M9: Linux** | PipeWire capture (audio + screen), AppImage + Flatpak packaging, Secret Service keychain, Linux CI |
| **M10: Windows** | Windows Graphics Capture, CUDA/ROCm builds, exe installer |

### Phase 2: Real-time + Polish (v2.0)

- Real-time transcription during recording
- Dashboard with stats and deadline tracking
- Calendar view

### Phase 3: Advanced (v3.0)

- Custom prompt template editor
- Speaker voice profiles across meetings
- Meeting scheduling integration
- Mobile companion app

---

## 10. Risks & Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| **PyInstaller sidecar is 2-4 GB** | Large download, slow install | High | Sidecar uses OpenCV headless to reduce size. Accept ~4-4.5GB install for full offline AI pipeline. |
| **Vulkan + MoltenVK untested for whisper.cpp** | Intel Mac + AMD GPU users fall back to slow CPU | Medium | Prototype early. CPU fallback always works. Document GPU requirements. |
| **pyannote model license incompatible** | Can't bundle model in MIT/Apache app | Low | Community model has permissive license. Review terms. If blocked, ship without diarization and require user to download model separately. |
| **ffmpeg GPL contamination** | License conflict with MIT/Apache project | Medium | Use LGPL-only ffmpeg build with hardware encoders. Verify at build time. |
| **Cross-platform capture divergence** | macOS and Windows capture APIs differ significantly | High | Abstract behind a trait. Implement macOS first (simpler API). Budget extra time for Windows. Accept some feature asymmetry initially. |
| **Whisper transcription quality on long recordings** | Accuracy degrades on 2+ hour recordings | Medium | Chunk audio into segments before transcription. Display confidence scores. Users can select larger models for better accuracy. |
| **Ollama not installed** | User has no LLM available on first launch | Medium | Clear in-app messaging: "Install Ollama for local AI, or add a cloud API key." Link to Ollama download page. Transcript is still useful without summary. |
| **PipeWire not available on older Linux** | Linux users on PulseAudio-only can't record | Medium | Document PipeWire requirement clearly. Target modern distros only (Ubuntu 22.04+, Fedora 34+). PipeWire adoption is near-universal on modern Linux. |
| **ChromaDB adds complexity to sidecar** | Sidecar is no longer thin, lifecycle management needed | Low | On-demand sidecar lifecycle with warm-up. ChromaDB persists to disk. Sidecar starts lazily and stays running while app is open. |

---

## 11. Constraints

1. **No accounts or authentication** — Locus has no user accounts, no login, no cloud backend. Everything is local.
2. **No telemetry by default** — No data collection unless the user explicitly opts in (and opt-in doesn't exist in v1).
3. **No subscription or payment** — Locus is free and open source. Revenue model (if any) is deferred to v3+ via open-core premium features.
4. **macOS 12.3+ minimum** — Required by ScreenCaptureKit. Older macOS versions are not supported.
5. **Windows 10 1903+ minimum** — Required by Windows Graphics Capture API.
6. **Python sidecar is opaque to the user** — The user never sees Python, pip, or any Python artifacts. The sidecar is a standalone binary.
7. **Linux requires PipeWire** — PulseAudio-only and bare X11 systems without PipeWire are not supported.
8. **No lite build** — The Python sidecar is required for core functionality (VAD, diarization, ChromaDB). A single full build is shipped per platform.

---

## 12. Glossary

| Term | Definition |
|------|-----------|
| **Diarization** | The process of identifying and labeling which speaker spoke when in an audio recording |
| **Whisper** | OpenAI's open-source speech-to-text model, used via the whisper.cpp C++ implementation |
| **whisper-rs** | Rust bindings for whisper.cpp, enabling native Rust integration without Python |
| **pyannote** | An open-source Python library for speaker diarization and voice activity detection |
| **Sidecar** | A secondary process that runs alongside the main Tauri app, in this case a PyInstaller-packaged Python binary |
| **JSON-RPC 2.0** | A stateless, lightweight remote procedure call protocol encoded in JSON, used for Rust ↔ Python communication |
| **Pipeline** | The sequence of processing steps applied to a recording: encode → transcribe → diarize → slides → OCR → summarize |
| **Ollama** | An open-source tool for running large language models locally on a user's machine |
| **ScreenCaptureKit** | Apple's macOS framework for capturing screen content and system audio (macOS 12.3+) |
| **Windows Graphics Capture** | Microsoft's API for capturing screen content on Windows 10+ |
| **OCR** | Optical Character Recognition — extracting text from images |
| **RAG** | Retrieval-Augmented Generation — enhancing LLM responses with relevant documents retrieved from a vector database |
| **Prompt routing** | Automatically selecting the appropriate LLM prompt template based on detected meeting type |
| **Meeting type** | Classification of a recording as either a "business meeting" (→ action items, deadlines) or a "lecture" (→ study notes, key concepts) |
| **ChromaDB** | An open-source vector database used for storing and querying document/transcript embeddings in the Knowledge Base |
| **EmbeddingGemma-300M** | A 300M-parameter embedding model from Google, used to convert text into vector representations for semantic search |
| **VAD** | Voice Activity Detection — identifying which portions of an audio recording contain speech vs. silence |
| **PipeWire** | A modern Linux multimedia framework for audio and video capture, replacing PulseAudio and JACK |
| **ONNX Runtime** | A cross-platform inference engine for running ML models exported in the ONNX format |
| **Semantic search** | Finding content by meaning rather than exact keyword matching, powered by vector embeddings and similarity search |

---

## Document Cross-References

| Document | Purpose | Location |
|----------|---------|----------|
| **PRD** (this document) | *What* and *why* — product requirements, personas, acceptance criteria | `docs/plans/PRD.md` |
| **DESIGN.md** | *How* — architecture, tech stack, data model, monorepo structure, build strategy | `docs/plans/DESIGN.md` |
| **SPEC.md** | *Engineering detail* — user stories, implementation decisions, testing seams, out-of-scope | `docs/plans/SPEC.md` |

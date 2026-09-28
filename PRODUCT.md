# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Tauri (React 19, TypeScript, Rust, Python sidecar, SQLite, Astryx + TailwindCSS, TanStack Query, TanStack Router, Zustand, Plyr / Video.js)

## Users

- **Knowledge Workers (e.g., Morgan)**: Product managers, engineers, designers, and team leads attending 4–8 meetings/week across Zoom, Google Meet, and Microsoft Teams. They require zero-friction recording (≤ 2 clicks), reliable capture of decisions and checkable action items with deadlines/assignees, and quick historical retrieval weeks later without manual note-taking.
- **Students & Researchers (e.g., Alex)**: University and online course students attending 2–4 lectures/week. They need synchronized video and transcripts, auto-extracted slide decks with OCR text, and structured key concept study summaries.

## Product Purpose

Locus is an open-source, offline-first desktop meeting recorder and knowledge library that transforms recorded conversations, screen shares, and slides into structured, searchable intelligence—executing all media capture, transcription, diarization, OCR, and summarization locally by default, with remote AI available only when a configured provider is explicitly selected as the destination for disclosed generation requests without requiring repeated prompts on every action.

## Positioning

Unlike cloud-based meeting recorders (such as Otter or Fireflies) that force sensitive enterprise audio onto third-party servers, require recurring monthly subscriptions, and lock users into proprietary silos, Locus executes transcription (whisper-rs), speaker diarization (pyannote), slide extraction (OpenCV + Tesseract), and local LLM inference (llama-server) entirely on the user's workstation. Data remains stored in standard open formats (MP4, SQLite, Markdown, JSON) with zero telemetry and zero mandatory internet access.

## Operating Context

- **Operating Systems**: Native desktop application running on macOS 13+ (ScreenCaptureKit), Windows 10 1903+ (Windows Graphics Capture + WASAPI), and Linux (PipeWire audio & portal).
- **Usage Environment**: Runs alongside active video conferencing applications or during in-person microphone capture; captures long sessions (up to 3 hours) with background post-recording processing.
- **Artifacts & Data Boundaries**: Local filesystem storage with configurable data directories; OS keychain for remote API credentials; SQLite for relational state; local ChromaDB for vector embeddings; standard H.264/AAC MP4 media files.
- **Capture Defaults & Controls**: Default capture sources are System Audio (on) + Screen (on) + Microphone (off). Microphone audio is diarized by default; an explicit “Only me on this microphone” setting may assign microphone speech to the User. Desktop window-close retains active capture with persistent controls, including a compact control-window fallback on environments lacking system tray support.

## Capabilities and Constraints

- **Offline Independence**: Bundled Whisper `small` model and pyannote community diarization operate on a disconnected first launch; skippable local model setup provisions GGUF summarization and embedding weights without blocking recording.
- **Privacy & Security Sovereignty**: Strictly zero telemetry or tracking; remote AI APIs (OpenAI, Anthropic, Gemini, remote Ollama) are activated only when explicitly selected as the destination for disclosed generation requests; saving credentials makes providers available but does not authorize generation; credentials never fall back to plaintext.
- **Pipeline Fault Tolerance**: Independent step state tracking (pending, running, done, error, blocked, skipped, canceled); crashes preserve recoverable media with target ≤5 seconds loss; failed steps retry without invalidating unrelated upstream work; previous summary revisions and their revision-owned action item completions are preserved during regeneration.
- **Desktop Ergonomics**: Persistent sidebar-driven navigation with background recording persistence across window close; high-precision video-to-transcript synchronization (<200ms seek latency); dedicated Slides tab with timestamped presentation slides and occurrences; checkable action items with revision-owned completion; cited multi-scope Knowledge Base chat incorporating transcripts, slide OCR, current summary, and linked documents.

## Brand Commitments

- **Name**: Locus.
- **Visual Identity**: Architectural, tactile, scholarly, and professional—drawing inspiration from precision studio recording gear, archival research libraries, and Swiss typography. Resolutely avoids ephemeral "vibecoded" tropes, glowing neon cards, and unstructured glassmorphism.
- **Color Philosophy (Editorial Botanical)**: Deep botanical ink (`#19211D` / `oklch(0.18 0.012 160)`) set against a warm, calming architectural paper-beige ground (`#FAF8F5` / `oklch(0.98 0.006 85)`) in light mode; deep obsidian slate (`#111614` / `oklch(0.15 0.01 160)`) in dark mode.
- **Signature Green Accents**: `#419873` (Primary Evergreen Accent), `#49ab81` (Interactive Emerald), and `#52bf90` (Luminous Mint) reserved for active recording states, audio visualizer meters, active playback transcript indicators, checkable completion states, and dark-mode focus rings—never degraded into low-contrast body text.

## Evidence on Hand

- `docs/plans/PRD.md`: Approved requirements, user personas, functional specifications, and acceptance criteria.
- `docs/plans/SPEC.md`: Observable behaviors, state machine invariants, hardware/UI validation contracts, and failure handling.
- `docs/plans/DESIGN.md`: Architectural boundaries, Tauri IPC specifications, SQLite schema, and sidecar protocols.
- `CONTEXT.md`: Canonical domain language definitions (Meeting, Recording, Capture source, Summary revision, Action item, Knowledge Base, Chat scope, Selected provider).

## Product Principles

1. **Local by default, cloud by choice**: Total privacy with zero silent network egress; remote providers are only utilized when explicitly selected as the destination with disclosed request content, without requiring a repeated prompt on every individual request.
2. **Capture everything, organize later**: Frictionless, resilient recording initiation (≤ 2 clicks) with intelligent post-meeting automated synthesis.
3. **Partial results are better than no results**: A failure in one downstream pipeline stage (e.g., slide OCR or LLM summarization) never impairs access to successful upstream outputs (raw video, audio, transcript).
4. **The user controls their data**: Direct ownership with standard open file formats, instant clipboard copies, full Markdown/JSON export, and clean deletion fencing.
5. **Smart defaults, full control**: Sensible automatic detection of meeting types, spoken languages, and hardware accelerators, always coupled with explicit user overrides.

## Accessibility & Inclusion

- Universal keyboard navigability across all views, controls, media playback, and transcript segments.
- Built-in ARIA semantics leveraging Astryx design system component standards.
- High contrast typography adhering to WCAG 2.1 AA/AAA (body copy contrast ≥ 13:1 vs. background; interactive controls ≥ 4.5:1).
- Non-reliance on color alone: status indicators combine color pills with textual badges and distinct iconography; speaker colors are always accompanied by textual labels.

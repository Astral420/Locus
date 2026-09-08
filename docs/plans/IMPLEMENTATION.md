# Locus v1 Implementation Plan

**Approved scope:** 2026-09-05 · **Execution status:** not started; repository contains planning documents, not the application.

## Authority and delivery rules

Read [PRD](./PRD.md) for requirements, [SPEC](./SPEC.md) for behavior/acceptance, [DESIGN](./DESIGN.md) for architecture, [CONTEXT](../../CONTEXT.md) for terminology and [AGENTS](../../AGENTS.md) for contributor gates. This plan sequences the agreed scope; it cannot override those contracts. Resolve demonstrated feasibility failures with evidence before changing product requirements.

Build the full feature set on macOS, then port Linux and Windows. Run platform feasibility spikes early to expose incompatible dependencies. Maintain one stable release channel; development artifacts are for engineering tests, not a maintained public beta. v1.0 requires qualification on all three platforms.

Checkboxes indicate implementation completion, not design approval. A task completes only when its artifacts exist, its stated acceptance checks pass, and its evidence is recorded. There are no completed implementation tasks at the time this plan is written.

## Model ownership and reasoning tiers

These are task assignments, not claims that a particular launcher already exposes every model. Resolve exact provider/launcher identifiers at execution time and preserve the requested model. If unavailable, report the blocked assignment; do not silently substitute.

| Work | Assigned model | Reasoning effort | Boundary |
|------|----------------|------------------|----------|
| All frontend work | Gemini 3.8 Flash | High | UX, design, React/TypeScript, styling, frontend scaffolding/configuration, typed wrappers, accessibility, frontend tests and visual verification; use impeccable |
| Orchestrator | GPT-6 Astra (`gpt-6-astra`) | medium for decomposition/integration decisions; low for routine dispatch/checkpoints | Own task packets, dependency order, evidence checks and escalation in oh my pi |
| Default backend executor | GPT-5.6 Luna (`gpt-5.6-luna`) | xhigh | Most backend implementation and tests, delivered as bounded TDD slices |
| Complicated backend executor | GPT-6 Astra (`gpt-6-astra`) or GPT-5.6 Sol (`gpt-5.6-sol`) | Astra low or Sol high | Unsettled native behavior, durability/concurrency design and difficult cross-module failures |
| Backend reviewer | GPT-5.6 Sol (`gpt-5.6-sol`) | medium for bounded changes; high for risk-sensitive contracts | Separate review session; validate behavior, tests, standards and evidence |

Luna xhigh is the default for 17 of 21 backend tasks. B00/B06 retain Astra low for native capture; B07/B15 retain Sol high for scheduling/publication and deletion. Their settled follow-up slices can return to Luna xhigh. B18/B19 start with Luna against the earlier feasibility contracts and escalate unresolved native behavior before proceeding.

Use [ORCHESTRATION](./ORCHESTRATION.md) when setting up oh my pi, dispatching work, checking progress, reviewing or resuming a run. That document defines checkpoint evidence and escalation. The orchestrator chooses between the approved Astra low / Sol high executor profiles when uncertainty exceeds a bounded Luna slice; record the reason and ownership change. Other models or reasoning tiers require maintainer disposition. Sol review uses a separate session even when Sol implemented the change.

All `F*` tasks below inherit Gemini 3.8 Flash High + impeccable at High. Backend agents do not take frontend work to finish a milestone. Backend owners define wire contracts/fixtures; Gemini owns frontend implementations and tests against them. Backend code owns capture and processing state; frontend stores own presentation state.

For impeccable, use the installed [skill](../../.agents/skills/impeccable/SKILL.md) and its applicable setup/playbook. PRD/SPEC/architectural DESIGN remain authoritative; generated visual briefs must not overwrite the architecture document or reopen approved product choices. Honor Operate-mode task clarity, the existing reference screenshots, keyboard access and bounded visual verification. Exact visual choices belong to Gemini's frontend task.

## Task handoff contract

Every task handoff includes:

- Task ID, owner/model/effort, prerequisites and allowed files/modules.
- PRD IDs and SPEC contracts covered, plus versioned input/output DTOs and error/state fixtures.
- Expected behavior, approved public test seams, one initial failing-test scenario, adverse scenarios, required commands and manual checks.
- Checkpoint record path, baseline revision, reviewer/model/effort and escalation triggers.
- Changed files, exact verification results, unverified hardware/dependencies and outstanding risks.

Backend contracts must be usable before frontend integration: provide representative success, empty, blocked, outdated, failure, canceled and deleted states. Generated TS types belong to the agreed contract-generation path; Gemini owns hand-written wrappers and consumers. Changes across a boundary require producer and consumer validation together.

Parallel work is safe only across explicit ownership boundaries and settled contracts. Do not edit a shared schema or protocol concurrently without one owner coordinating revisions. Each merged slice must preserve already working functionality.

## TDD and orchestrator checkpoints

All behavior implementation in B* and F* tasks follows the installed [TDD skill](../../.agents/skills/tdd/SKILL.md): one public behavior test, observed red, minimal implementation, observed green, then the next slice. Use SPEC's agreed testing seams and task-specific acceptance contracts; record existing approval in the task packet and obtain maintainer agreement before adding a new seam. Refactoring belongs to the review stage and must preserve passing behavior tests.

Every task passes the [checkpoint protocol](./ORCHESTRATION.md#checkpoint-protocol): C0 scope/seams, C1 first red, C2 slice green, C3 independent review and C4 task/phase acceptance. The executor records every red/green cycle; the orchestrator inspects the first red before implementation and each completed slice before releasing another. Repeat C1 when the seam, contract or risk changes. A task's **Gate** below remains mandatory in addition to these checkpoints.

Documentation-only work uses link/consistency checks. Scaffolding first establishes a runnable test harness; feasibility spikes record hypotheses and measured experiments where a meaningful automated red is unavailable. These exceptions are recorded at C0 and do not exempt production behavior from TDD. Manual capture/GPU/UI qualification supplements automated contracts. CI commands become executable only when the relevant scaffold exists.

At every phase boundary, the orchestrator reconciles completed task evidence, unresolved feasibility gates, backend/frontend contract compatibility and next-phase dependencies. Release only tasks whose own prerequisites pass; independent work may continue around a recorded blocker.

## Phase 0 — Prove prerequisites

### B00 — Native capture and recoverable encoding feasibility

- **Owner:** GPT-6 Astra · low.
- **Depends on:** approved docs; actual target hardware/VM access for qualification.
- **Deliver:** bounded prototypes demonstrating macOS 13 system audio + microphone + optional video, common timestamps, continuous recoverable segments, final H.264/AAC MP4 and audio-only playback. Establish minimal Windows WGC/WASAPI and Linux PipeWire/portal feasibility before platform-specific assumptions become dependencies.
- **Gate:** demonstrate pause offsets, forced process termination at varied segment boundaries, ≤5-second tail-loss target under recorded storage assumptions, encoder failure and unavailable-source handling. Record H.264 hardware/fallback encoder availability and exact FFmpeg build provenance. List inaccessible platforms as unverified.

### B01 — Offline AI packaging and backend qualification

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B00 format contract; may investigate dependencies concurrently with B00.
- **Deliver:** pinned compatible Rust/whisper-rs, Python/PyTorch/pyannote/ChromaDB/PyInstaller and Tesseract dependency set; bundled standard Whisper small, pyannote assets and OCR data; managed llama-server candidates and compatible generation/embedding catalog entries.
- **Gate:** clean-machine, network-disabled smoke run for every bundled feature, no developer caches/token/Python install; CPU baseline; measured packaged/installed size; exact model/code notices and redistribution verification. Validate supported GPU paths and failure-to-CPU behavior. Separate AMD Metal PR #19527 from MoltenVK and justify any pinned fork with measurements.
- **Output:** checked-in feasibility results and reproducible artifact recipes in implementation documentation. Do not silently drop bundled features if a gate fails.

### F00 — Frontend/runtime feasibility and UX foundations

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** approved surface/behavior docs and B00 sample media.
- **Deliver:** verify Astryx's official package and React/Tauri compatibility; select/pin Plyr or Video.js after keyboard/seek/audio-only checks; establish visual tokens and desktop app shell direction using the screenshot references.
- **Gate:** media/transcript seek works in target webviews; accessible source controls and narrow-window layouts are viable; no frontend package/visual choice weakens capture or privacy contracts.

## Phase 1 — Foundation, contracts and contributor enforcement

### B02 — Rust/Python foundation and persistent ownership

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B00–B01.
- **Deliver:** Rust domain/commands/db boundaries, rusqlite DB worker and incremental migration framework, Python sidecar handshake/JSON-RPC client/server, owned processes, app-data root resolution and keychain adapter. Initial schema includes stable ownership/revision/job identities; feature-specific migrations follow their owning tasks.
- **Gate:** real SQLite tests for foreign keys/transactions/migration rollback, clean restart and separate owners; sidecar framing/version mismatch/malformed input/crash/cancel tests; redacted credentials and keychain-unavailable behavior. No frontend access to arbitrary paths or shell commands.

### B03 — Public API and deterministic contract fixtures

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B02 and DESIGN communication/data contracts.
- **Deliver:** versioned commands, DTOs, events, structured errors and type-generation convention for capture, jobs/revisions, models, meetings, KB and settings. Test fixtures cover all non-happy states; frontend has no invented parallel state machine.
- **Gate:** serialization/validation tests on producer and sidecar boundaries; event IDs and stale-attempt rejection; explicit pagination/cancel semantics. Agree the generated-type ownership boundary with Gemini.

### F01 — Frontend scaffold and contract consumption

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F00, B03 contracts/fixtures.
- **Deliver:** React/TypeScript setup, Astryx/Tailwind, TanStack Router/Query, Zustand presentation state, five sidebar items plus meeting detail, error boundaries, typed wrappers and frontend test harness.
- **Gate:** typecheck/lint/build pass; query resynchronization on reopen/refocus; keyboard navigation; fixtures exercise empty/loading/error states without backend implementation assumptions.

### B04 — Contributor and CI guardrails

- **Owner:** GPT-5.6 Luna · xhigh. Gemini owns frontend scripts/tests used by the workflows.
- **Depends on:** B02–B03 and F01 tooling.
- **Deliver:** PR workflows for backend lint/format/build/CPU tests, sidecar tests, frontend checks/build/critical-flow tests, schema/contract and Markdown-link validation. Add the original-code MIT LICENSE with verified copyright attribution and a separate third-party notice inventory. Add PR template requiring problem/requirement references, validation evidence and remaining risks; configure reviewer ownership with actual maintainer identities, never invented usernames.
- **Gate:** deliberately failing contract/behavior checks fail CI; deterministic jobs require no cloud keys and cannot execute untrusted fork code with privileged secrets. Required-check names are stable. Document maintainer setup for protected default branch, required status checks and review, resolved conversations and protection against bypass.
- **Authority:** prepare local workflow/configuration artifacts as implementation work; changing remote repository settings or publishing/merging remains a maintainer action. Record configuration evidence before declaring remote enforcement complete.

## Phase 2 — Model setup and durable recording

### B05 — Managed models and inference process lifecycle

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B01–B03.
- **Deliver:** role-aware Whisper/generation/embedding catalog with exact revisions/checksums, resumable verified downloads, compatible local import into managed storage, bundled-model lookup, explicit select/delete, process health/authentication and memory leases. Generation and embedding process roles cannot accidentally use the wrong loaded model.
- **Gate:** offline/cancel/resume/corrupt hash/unsupported model/low disk tests; missing external drive leaves bundled capture/transcription available; active models cannot be deleted. No implicit network model loading or remote fallback.

### F02 — Setup, Model Manager and downloads UX

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F01, B05 contracts; use fixtures before live integration.
- **Deliver:** skippable setup showing model sizes/download/import, unified Model Manager and persistent footer progress, readiness and actionable retry/cancel; selected versus installed/configured states remain clear.
- **Gate:** a disconnected first launch reaches recording; a canceled setup can be completed later; errors survive navigation and success does not create a toast storm.

### B06 — Production capture lifecycle and recovery

- **Owner:** GPT-6 Astra · low.
- **Depends on:** B00, B02–B03.
- **Deliver:** native macOS capture adapters, one Rust-owned active capture, durable segments/manifest, common media clock, source/mixed tracks, pause/resume, finalization/recovery, preflight headroom, background system controls and explicit quit handling.
- **Gate:** SPEC capture matrix passes: all-off/screen-only rejected; mic-only/in-person/audio-only supported; close/reopen, double commands, pause drift, source loss, disk full, forced sleep and process crash. Three-hour warning does not stop capture. Prove target tail-loss with playable recovered media.

### F03 — Recording, system-control presentation and recovery UX

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F01, B06 contracts; backend owns native control actions/lifecycle.
- **Deliver:** sources/device selection, Only me on microphone setting, title/type/language inputs, timer/audio levels, pause/stop, recovery and permission flows, persistent background-state presentation.
- **Gate:** critical interaction tests reject invalid starts and stale UI transitions; closing/reopening reflects Rust state; interruptions never appear as successful recordings or silently resume.

## Phase 3 — Processing and meeting review

### B07 — Durable scheduler and atomic artifact publication

- **Owner:** GPT-5.6 Sol · high.
- **Depends on:** B02–B03, B05–B06.
- **Deliver:** durable job/attempt graph, Balanced/Maximum budgets, capture priority/resource leases, terminal optional dependencies, cancellation, retries, revision invalidation and atomic publication with deletion fencing.
- **Gate:** competing recording/inference, non-cooperative cancellation, crash/restart, duplicate/late events and two-attempt races; no duplicate artifacts or erased last success. Missing models block only their consumers.

### B08 — VAD, transcription and source-aware diarization

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B01, B06–B07.
- **Deliver:** mixed audio normalization, VAD-guided ≤5-minute chunks, sequential fallback, full media-time mapping, boundary deduplication, automatic/manual language, source speaker alignment and stable speaker identity/name references.
- **Gate:** no-speech and failed VAD differ; multi-person mic versus explicit User mode; overlap/echo uncertainty, chunk boundaries and pauses; bundled CPU/GPU fixtures and memory-bounded three-hour tests with measured accuracy/timing.

### B09 — Slides and OCR

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B01, B06–B07.
- **Deliver:** stable slide detection/deduplication with repeated occurrences, timestamped images, bounded OCR and available text publication.
- **Gate:** absent video, no slides, repeated slide, noisy transitions, partial OCR failure and missing language data; images remain viewable and speech work proceeds. No unbounded frame accumulation.

### B10 — Meeting queries and speaker updates

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B02–B03, B08–B09.
- **Deliver:** paginated meeting/title search/type/date filters, detail projections, current/outdated results, stable speaker rename and revision-aware playback data.
- **Gate:** date/timezone boundaries, absent media/results, pagination, rename across structured references and query exclusion of tombstoned owners; no global free-text name replacement.

### F04 — Meetings and synchronized media review

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F01, B08–B10 contracts.
- **Deliver:** searchable/filterable list, audio/video playback, timestamp seek/highlight, transcript navigation, speaker rename, slides gallery, persistent per-step status and retry.
- **Gate:** keyboard/seek/long transcript performance; no-video/no-speech/optional-failure states; failed retry keeps prior results visible. Bounded visual checks on shipped desktop sizes.

## Phase 4 — Selected providers, summaries and titles

### B11 — Provider adapters and authorization

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B03, B05, B07.
- **Deliver:** local llama-server, selected Ollama and OpenAI/Anthropic/Gemini adapters, redacted credential config, remote endpoint classification, streaming/cancellation and request-context disclosure metadata.
- **Gate:** merely saved keys/detected Ollama send no content; explicit selection snapshot controls each request; authentication/timeout/rate-limit/cancel errors never activate another destination. Deterministic tests mock providers; opt-in live qualification is separate.

### B12 — Grounded long-input summaries, revisions and automatic titles

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B07–B11.
- **Deliver:** meeting/lecture/generic routing and versioned templates, context-budgeted full-session summarization, validated structured sections/actions/citations, explicit regeneration, immutable prior summary/action state and title-origin compare-and-set.
- **Gate:** three-hour continuous transcript coverage without silent truncation; unknown deadlines/assignees; failed optional inputs; invalid model output; checked items survive regeneration; user rename beats in-flight title generation; short sessions work and unavailable providers leave placeholders.

### F05 — Summary, action items and provider selection

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F04, B11–B12 contracts.
- **Deliver:** cited summaries, checkable actions, prior revision view, outdated/retry/regenerate states, meeting-type override and explicit local/remote provider selection with clear destination.
- **Gate:** no hidden automatic replacement of reviewed content; key storage is distinct from selection; citations seek correct sources; title edits do not flicker back after generation finishes.

## Phase 5 — Knowledge Base, provenance and deletion

### B13 — Document ingestion and source indexing

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B03, B05, B07–B09.
- **Deliver:** limited text PDF/MD/TXT ingestion, page/text provenance, exact-byte deduplication, independent document ownership/links, durable source/chunk jobs, explicit vectors to ChromaDB and model-versioned index generations.
- **Gate:** 50 MiB/500-page boundaries, encrypted/image-only/mixed PDF and invalid text, duplicate links, summary-independent transcript indexing, restart/retry, changed embedding dimensions and atomic index switch. Test actual pinned vector storage, not only a mock dictionary.

### B14 — Fixed-scope chat and citation provenance

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B11–B13.
- **Deliver:** persistent scoped threads/messages, retrieval filtering before ranking, prompt budgeting, validated citations, source/turn dependencies and incomplete streaming states. Treat source instructions as untrusted data.
- **Gate:** scope never leaks across meetings; new scope gets new thread; this-meeting includes linked docs; empty/insufficient evidence, hallucinated citation IDs, provider switches, canceled streams and adversarial document content.

### B15 — Deletion across stores and in-flight work

- **Owner:** GPT-5.6 Sol · high.
- **Depends on:** B07, B10, B12–B14.
- **Deliver:** transactional visibility tombstones/job generation fences, durable physical cleanup, owned artifact/vector removal, independent-document preservation, scoped-chat deletion and transitive mixed-chat cleanup.
- **Gate:** race deletion against transcription/indexing/streaming/retry; restart halfway through cleanup; verify no new retrieval/export visibility or late publication, no surviving dependent chat context and no deletion of shared documents/unrelated user files. Inject sidecar/storage failures to prove cleanup remains retriable.

### F06 — Knowledge Base and scoped Q&A

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F01/F05, B13–B15 contracts.
- **Deliver:** chat-first KB/thread sidebar, semantic search, document management/linking, per-meeting Q&A, citations, explicit scope-to-new-thread transition, model selector, indexing and deletion feedback.
- **Gate:** good keyboard/focus behavior on thread changes; previous scope context is absent; upload failures are actionable; indexing is seamless but discoverable; deleted-source/incomplete-answer states never masquerade as successful responses.

## Phase 6 — Settings, relocation, export and updates

### B16 — Safe data/model relocation

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B02, B05, B07, B13, B15.
- **Deliver:** separate model and meeting-data roots, idle preflight, migration manifest/copy/checksum/atomic switch/recovery, old-managed-copy cleanup and missing-drive handling.
- **Gate:** insufficient space, permission failure, drive unplug/replug, crash before/after switch, active-model blocking and complete SQLite/Chroma/media consistency. User-imported originals and unrelated folder contents survive.

### B17 — Export/settings utilities and stable updates

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** B10–B12, B15–B16.
- **Deliver:** clipboard/Markdown/PDF/JSON export, safe structured rendering with revision/provenance, recording defaults/storage usage, stable signed-update checks and idle installation gates.
- **Gate:** partial/outdated result exports, escaping/untrusted Markdown and no remote-resource loading, valid JSON/PDF, key redaction; failed signature/update during capture/migration rejected; package-managed installs follow supported update mechanisms.

### F07 — Settings, export and desktop polish

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** F02–F06, B16–B17 contracts.
- **Deliver:** three-column settings, appearance/defaults/privacy/providers/storage, relocation UI, exports and stable-update presentation; final accessibility and bounded visual passes.
- **Gate:** keyboard navigation, contrast/non-color status cues, narrow windows, disabled versus unavailable actions, migration progress/recovery, successful export feedback and explicit update action. Required frontend checks pass.

## Phase 7 — Linux and Windows production ports

### B18 — Linux capture and delivery

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** completed macOS core B00–B17 and early Linux feasibility.
- **Deliver:** PipeWire/portal adapters, Secret Service, supported GPU/encoder paths, AppImage/Flatpak bundles, discoverable background controls and platform-native update handling.
- **Gate:** repeat recording/recovery/permission/CPU-fallback/offline and storage tests on named distro/desktop configurations; validate actual capability instead of assuming PipeWire by distribution name. Record clean install/upgrade evidence.

### B19 — Windows capture and delivery

- **Owner:** GPT-5.6 Luna · xhigh.
- **Depends on:** completed macOS core, early Windows feasibility; production delivery follows Linux in the agreed sequence.
- **Deliver:** WGC video + WASAPI loopback/mic, clock/device switching, Credential Manager, qualified CPU/GPU/encoder variants, signed installer and updates.
- **Gate:** full source/pause/recovery matrix, multi-device/driver failure, clean-machine offline use, playback, install/upgrade and verified target OS compatibility. Never infer audio support from WGC alone.

### F08 — Platform frontend adaptation

- **Owner:** Gemini 3.8 Flash High · High · impeccable.
- **Depends on:** B18/B19 platform contracts and existing F01–F07 UI.
- **Deliver:** native shortcut/window/permission presentation and webview compatibility fixes for each port while preserving established interactions.
- **Gate:** critical frontend suite plus keyboard/media/system-control manual checks on each platform; no unrelated redesign during porting.

## Phase 8 — Release qualification

### B20 — Release evidence and artifact audit

- **Owner:** GPT-5.6 Luna · xhigh. Gemini owns remaining frontend defects/validation.
- **Depends on:** every prior applicable task and CI/review enforcement configuration.
- **Deliver:** reproducible signed artifacts, separate third-party notices/source provenance, offline clean-install and upgrade evidence, schema migration/recovery evidence, verified stable updater/package-manager behavior, release notes and known hardware limits.
- **Gate:** required PR checks on the final revision; three-platform capture/playback/recovery; three-hour continuous/silence-bearing benchmarks on defined 8GB/CPU/GPU reference hardware; no unverified requirement advertised as supported. Record measured sizes and timing against PRD targets, with maintainer disposition for unmet targets.
- **Release authority:** preparing artifacts is not permission to publish, merge or change remote settings. Maintainer performs or explicitly authorizes those actions. No separate public beta channel is created.

## Required scenario-to-task traceability

| Product requirements | Primary owners/tasks | Acceptance evidence |
|----------------------|----------------------|---------------------|
| FR1, FR12.1–5 | B00/B06, F03 | source combinations, pause clock, background controls, interruption/crash recovery, three-hour warning |
| FR2–4 | B01/B08/B09, F04 | model/CPU/GPU fixtures, language/timestamps, VAD fallback/no speech, speaker uncertainty, slide/OCR partial failures |
| FR5, FR12.6/9/12/13 | B11/B12, F05 | selected destination only, long-input coverage/citations, retained revisions/checkmarks, conditional title publication |
| FR6–7, FR12.7 | B10/B15, F04/F06 | filters/playback, current/outdated states, concurrent delete/retry and shared document protection |
| FR8–9, FR12.11 | B16/B17, F07 | partial exports, safe rendering, migration failure/recovery and missing drives |
| FR10, FR12.8–10/14 | B05/B13/B14/B15, F02/F06 | offline setup, independent index readiness, scopes/citations, dedup/limits, cleanup and index model switch |
| FR11, NFR compatibility/privacy/accessibility | B04/B18–B20, F01/F08 | required CI/review rules, stable updates, platform QA, local/offline boundaries and keyboard access |

## Definition of done for each pull request

1. Describe the concrete behavior change and link the applicable requirement/SPEC contract; keep the diff within that change and existing conventions.
2. Supply red/green evidence for each changed behavior at the agreed public seam, including meaningful normal/failure paths and an automatable bug regression before its fix. Record documentation/scaffold/spike exceptions and manual qualification separately.
3. Run all applicable repo-defined checks and record exact commands/results. Preserve assertions and existing functionality; no blanket skips, hidden failures or unrelated cleanup to make CI green.
4. Include manual hardware/UI evidence where automation cannot establish the result. Mark unavailable checks as unverified and obtain maintainer disposition before merge.
5. Complete C0–C4 with independent review findings resolved and evidence tied to the final revision. Update affected docs/contracts together; assess migrations, privacy/data egress, cancellation/retry and resource ownership for changed seams. Required checks and human review must pass on the final PR revision.

The existing product questions are resolved. Remaining dependency/artifact choices are implementation gates with evidence, owners and downstream dependencies above; they are not license to reopen approved scope or silently substitute assigned models.

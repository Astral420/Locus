# Native Capture Backends — Handoff Plan

**Status:** proposal for maintainer verification. Not yet part of IMPLEMENTATION.md.
**Audience:** an executing agent with no prior context, plus the maintainer who verifies it.
**Written:** 2026-10-03, against branch `dev` (last commit before this work: `2d1fdaa`).

> Authority order (from AGENTS.md): PRD governs what/why; SPEC governs observable behavior and
> acceptance; DESIGN governs architecture; IMPLEMENTATION sequences delivery. This plan does not
> override them. Where the current code deviates from them, this plan says so and proposes the
> path back to the documents. Proposed task IDs below (`NC-n`) must be mapped by the maintainer
> onto `M2.xx` / `M10.xx` before dispatch.

---

## 0. Rules for the executing agent

These come from `AGENTS.md` (repo root) and apply to every task below.

1. **One task at a time.** Finish the assigned task, record evidence, stop. Do not start the next
   task automatically. Respect each task's `Depends`.
2. **Preserve existing edits and working functionality.** The FFmpeg-based screen capture works on
   the maintainer's Mac today. It stays as a selectable fallback until a native backend is proven.
3. **Stay in scope.** No unrelated refactoring or speculative abstractions.
4. **Frontend work** (source/window picker UI, permission messaging, recovery UI) is routed to the
   frontend owner designated in IMPLEMENTATION.md / AGENTS.md "Model routing". Do not do it here;
   specify the Tauri command contract and stop at that boundary.
5. **Report exact results.** Run format, lint, type, build and tests on the final revision.
   Distinguish pre-existing failures. **Mark every hardware, OS-permission or desktop-session check
   you could not perform as `UNVERIFIED`** (SPEC §Testing: capture needs real hardware and is
   tested manually per platform during release QA).
6. Keep the Windows/Linux/macOS code compile-gated with `#[cfg(target_os = ...)]`. Fast CI must keep
   passing on all three runners.

---

## 1. Goal and decision

The maintainer's decision (2026-10-03), which matches the documents:

| Platform | Video | Audio | Encoding |
|---|---|---|---|
| macOS | **ScreenCaptureKit** | system audio via ScreenCaptureKit; microphone on a separate path | bundled **FFmpeg**, encode/mux only |
| Windows | **Windows Graphics Capture** | **WASAPI** loopback + microphone | bundled FFmpeg, encode/mux only |
| Linux | **PipeWire screen-cast portal** | PipeWire audio capture | bundled FFmpeg, encode/mux only |

FFmpeg is **not** used to capture the screen in the target design. It is used for H.264/AAC
encoding and MP4 writing/finalization, as the documents specify.

---

## 2. Source-of-truth map

| Topic | PRD | SPEC | DESIGN | IMPLEMENTATION |
|---|---|---|---|---|
| Native capture per OS | FR1.12, FR1.14, NFR10, NFR11, NFR12a | §Architecture "Capture" (l.139–140), §Capture and recovery (l.226–230) | Backend table (l.116–117), module tree `capture/{macos,windows,linux,encoder}.rs` (l.388–393) | M2.02, M10.10, M10.11 |
| Encode with bundled FFmpeg; H.264/AAC MP4; audio-only without dummy video | FR1.11, NFR12 | l.141, l.336 | l.118, l.586 | M2.03 |
| Separate system/mic tracks | **FR1.13** | l.165 | — | M2.03 ("separate audio tracks (mic, system, mixed)") |
| Source toggles / defaults / at least one audio source | FR1.4, FR1.5, FR12.1 | — | — | M2.06 |
| Screen = full screen **or specific window** | FR1.3 | — | — | — |
| Pause excluded from output; common monotonic clock | FR1.10, FR12.5 | l.229 | l.116 ("share one media clock") | M2.04 |
| Crash recovery, ≤5 s tail loss, never auto-restart | FR12.3 | l.228, l.230 | — | M2.05 |
| Window close keeps capture running; tray controls | FR12.4 | l.228 | — | M2.01/M2.07 |
| Source loss / permission revocation / disk full stop and preserve | FR12.3 | l.229–230 | — | M2.06 |
| FFmpeg exact minimal build, encoder fallback, notices | — | l.141, l.336 | l.586, l.601 | — |
| Platform QA is manual | — | l.298, l.335 | l.579–580 | — |

---

## 3. Current state and findings

### 3.1 What exists (branch `dev` plus the maintainer's accepted interim changes)

`src-tauri/src/capture/`:

| File | Role today |
|---|---|
| `mod.rs` | Capture state machine and manager (idle→recording↔paused→finalizing→saved, interrupted/recoverable). Starts audio and screen **in parallel**, owns pause/resume/finalize ordering, writes `sync-report.txt`. |
| `audio.rs` | `cpal`-based capture: one thread per source, WAV per source (`system_audio.wav`, `microphone.wav`), shared pause flag, per-track real start time, **aligned mix** to `audio.wav` (mono, 48 kHz). |
| `screen.rs` | **Interim, FFmpeg-driven screen capture.** Spawns FFmpeg (AVFoundation on macOS, gdigrab on Windows, unsupported on Linux), writes fragmented H.264 MP4 segments (a new segment per pause), waits for the first frame via `-progress`, then muxes segments + mixed audio into `recording.mp4`. Contains `locate_ffmpeg`, `prewarm`, `mux_recording`, `normalize_segment`, `describe_streams`. |
| `encoder.rs` | Manifest/segment bookkeeping (`Manifest`, `Segment`, `Encoder::finalize`, `recoverable_manifests`). **Not used by the manager** (see F5). |
| `timeline.rs` | Elapsed-time clock with pause exclusion (`MediaTimeline`). |
| `macos.rs`, `windows.rs`, `linux.rs`, `preflight.rs` | Preflight only. `windows.rs` is a stub (checks `WINDIR`); `macos.rs` gates system audio on macOS 14.6+. |

Related: `commands/recording.rs` (async commands on the blocking pool, `prewarm_capture`),
`src/stores/recordingStore.ts` (poll-race guard), packaging for a bundled FFmpeg
(`src-tauri/ffmpeg/`, `packaging/*.sh`, `.github/workflows/{build-ffmpeg,release}.yml`).

Verification status of the interim work: 53 Rust lib tests and 53 frontend tests pass; `cargo clippy
--all-targets -D warnings` and `cargo fmt --check` were clean on Linux. The maintainer confirmed on
macOS Tahoe 26.5.2 (Homebrew FFmpeg 9.0.2, `pnpm tauri dev`) that video playback works and the
black-screen start is gone.

### 3.2 Findings that drive this plan

**F1 — AVFoundation first-frame latency (~2 s).** The maintainer's `ffprobe` of a recording showed
`Video ... start 1.963997` and no start offset on audio. FFmpeg's AVFoundation input stamps frames
relative to when the device was opened, and the first frame arrived about two seconds later. The
interim code waits for the first frame (instead of a fixed 1.5 s sleep), parallelises device
start-up, and normalises timestamps at encode time (`setpts=PTS-STARTPTS`) and mux time
(`setts=ts=TS-STARTDTS`). **The root cause was inferred from the ffprobe numbers and could not be
reproduced** in the Linux sandbox: FFmpeg 6.1 and a current master build both normalise first
timestamps for synthetic (`lavfi`) inputs, and only a real capture device leaves them raw. Resume
after a pause pays the same ~2 s. Native capture removes this class of problem.

**F2 — Cross-stream alignment was wrong.** Audio tracks started sequentially but were mixed from
their own sample 0, so the later-starting microphone was early relative to system audio. Fixed in
`audio.rs` (per-track start times, silence padding in `mix_tracks_with_leads`). The native design
must keep a single media clock instead of reconciling after the fact (SPEC l.229).

**F3 — macOS version requirement deviates from the PRD.** NFR10 and DESIGN l.596 require macOS 13+
using ScreenCaptureKit system audio. The code uses a CoreAudio process tap via `cpal` loopback and
`macos.rs` rejects system audio below **macOS 14.6**. Returning to ScreenCaptureKit audio restores
the documented minimum. (Microphone capture inside ScreenCaptureKit is newer than macOS 13; the PRD
already says "separate microphone path where required". **Verify the exact availability.**)

**F4 — FR1.13 / FR12.2 are not met.** The PRD requires system and microphone audio as **separate
tracks, not mixed**. FR12.2 diarizes microphone speech by default. The final `recording.mp4`
contains **one mixed mono AAC stream** (confirmed in the maintainer's ffprobe: a single
`Audio: aac (LC), 48000 Hz, mono`). The per-source WAVs exist on disk but are not part of the MP4
or the manifest. M2.03's acceptance asks for "separate audio tracks (mic, system, mixed)".

**F5 — Recoverability (FR12.3, M2.05) is not implemented end to end.** `encoder.rs` has a manifest
and `recoverable_manifests`, but nothing in `mod.rs`/`lib.rs` uses them, and there is no start-up
recovery offer. Video segments are fragmented MP4 (decodable after a kill), but audio is WAV written
by `hound`, whose header is only finalised on stop; a crash leaves a WAV with an unfinalised
header. Also, successful mux deletes the raw segments.

**F6 — Screen-only is rejected, as FR12.1 requires.** The manager enforces at least one audio
source. Keep this.

**F7 — Screen target is the primary display only.** FR1.3 says "full screen **or specific
window**". There is no source/window picker and no display selection.

**F8 — The post-stop pipeline may not read the recorded files.** `pipeline/orchestrator.rs`
`enqueue` does not reference media paths in the code reviewed. **Unverified; confirm** before
claiming end-to-end transcription works. Out of scope here, but it constrains where separate audio
tracks must live (SPEC l.165: Rust sends "the audio file path" to the sidecar).

**F9 — Linux audio.** The PRD requires PipeWire audio (NFR12a). `cpal` on Linux uses ALSA by
default, which works through PipeWire's ALSA shim but is not the documented PipeWire capture
path. Decide explicitly (see D5).

**F10 — Windows adapter is a stub.** `windows.rs` only checks for a Windows session. `cpal` uses
WASAPI on Windows (loopback is supported on output devices) but this is **unverified on Windows**.

**F11 — Storage paths (pre-existing, not changed).** The DB and `media` folder use paths relative to
the launch directory (`media_root = "media"`), which works under `tauri dev` but will likely fail
in an installed build. `locus.sqlite*` files are committed to the repo. Flag to the maintainer;
fix is a separate task.

**F12 — FFmpeg encoder/licence is an open gate.** The interim path uses `libx264` (GPL). SPEC l.141
and l.336 list "usable hardware and distributable fallback encoders" and the exact minimal build as
feasibility gates; DESIGN l.586 says to verify hardware encoder paths and a distributable fallback.

**F13 — UI/command boundary issues already fixed in the interim work** (keep): commands run on the
blocking pool (they froze the UI as sync commands), poll-race guard in `recordingStore.ts`, REC pill
formatter bug (`00:5.617268483`), errors surfaced on the Record page.

### 3.3 Keep / replace

**Keep:** manager and state machine, async commands, audio parallel start and aligned mixing,
pause/resume ordering (stop video and audio at the same instant; resume waits for video), final
mux concept, FFmpeg bundling infrastructure and CI scripts, strict-FFmpeg test mode
(`LOCUS_REQUIRE_FFMPEG=1`), sync report.

**Replace (behind a backend switch):** the part of `screen.rs` where FFmpeg *captures* (AVFoundation
/gdigrab inputs, device-index discovery, `prewarm` device listing, first-frame detection through
`-progress`, the `-itsoffset`/`-ss` start-offset logic). Keep it as the `ffmpeg` fallback backend
until each native backend is proven.

Rough scale: about a fifth to a third of the capture code is replaced; the manager changes little
because `ScreenCapture`'s start/pause/resume/stop surface can remain the same.

---

## 4. Target architecture

```
 Platform adapter (video)            Shared media clock               Encoding (bundled FFmpeg)
 ┌──────────────────────┐         ┌──────────────────┐         ┌─────────────────────────────┐
 │ macOS  ScreenCaptureKit │──┐     │ MediaClock (mono │         │ rawvideo on stdin           │
 │ Win    Graphics Capture │──┼────▶│ origin, pause    │────────▶│  → H.264 fragmented MP4     │
 │ Linux  PipeWire portal  │──┘     │ intervals removed)│        │  segment per pause/rotation │
 └──────────────────────┘ frames    └──────────────────┘         │ audio → AAC                 │
 Platform adapter (audio)            + pts from clock             │ final: MP4 (+faststart)     │
 ┌──────────────────────┐ PCM+pts            ▲                    └─────────────────────────────┘
 │ SCK audio / WASAPI /  │───────────────────┘                            ▲
 │ PipeWire; mic path    │                          Frame pacer (CFR 30 fps: repeat/drop) ─┘
 └──────────────────────┘
```

### 4.1 Contracts (sketch; the agent finalises names in `capture/`)

- `VideoFrame { data, width, height, stride, pixel_format (NV12 preferred, BGRA fallback), pts }`
  where `pts` is on the shared media clock.
- `trait ScreenSource: Send` with: `start(target) -> Result<FirstFrameInstant>` (returns when the first
  frame exists), `pause()`, `resume()`, `stop()`, and a frame callback/channel. Each adapter owns its
  OS threads; the trait must be implementable by all three adapters and by a **synthetic test source**.
- `MediaClock`: single monotonic origin shared by video and every audio source; pause intervals are
  subtracted; all offsets reported to the DB/transcript use it (SPEC l.229).
- `FramePacer`: produces constant 30 fps. Capture APIs send frames only when the screen changes
  (ScreenCaptureKit reports idle frames), so the pacer repeats the last frame to fill gaps and drops
  extras. Acceptance: output frame count equals duration × fps ±1 over a long run (no drift).
- `VideoEncoder` (`encoder.rs`-adjacent): spawns FFmpeg with `-f rawvideo -pix_fmt nv12 -s WxH
  -framerate 30 -i pipe:0`, writes fragmented MP4 (`frag_keyframe+empty_moov+default_base_moof`) and
  **registers every segment in the durable manifest** (see NC-5).

### 4.2 Design notes and risks to resolve (do not assume; verify)

- **Pipe bandwidth.** 1080p BGRA at 30 fps is ~250 MB/s; NV12 is ~93 MB/s. Prefer asking the OS for
  NV12 and scaling in the capture API (cap output at a configurable resolution, default ≤1080p).
  Retina/HiDPI sources are 2–4× larger than 1080p.
- **Encoder choice** is decision D1. Candidates: `libx264` (GPL), `h264_videotoolbox` (macOS),
  Media Foundation `h264_mf` / vendor encoders (Windows), OpenH264 or `libx264` fallback (Linux).
  Whatever is chosen needs a distributable software fallback (SPEC l.336).
- **Display changes** mid-recording (resize, hot-plug, resolution switch, rotation): define behaviour
  (restart segment with new size vs scale to fixed size). Must not silently drop video (SPEC: never
  silently downgrade selected sources).
- **DRM/protected content** can yield black frames. Document; do not treat as a capture failure.
- **Permission revocation / sleep / source loss / disk full** must stop capture and preserve media
  (FR12.3, M2.06). Test each as an interruption case.
- **Backpressure.** If FFmpeg stalls, drop frames at the pacer, never block the capture callback.
  Memory must stay bounded for 3-hour sessions (M2.08).
- **3-hour size.** The interim encode is about 1.1 Mb/s for a 1080p screen (~1.5 GB per 3 h);
  confirm disk headroom reservation in preflight.

### 4.3 Candidate libraries (**unevaluated here; the agent must check current versions, maintenance, licences and macOS/Windows/Linux build requirements and record the choice**)

- macOS: a ScreenCaptureKit binding such as the `screencapturekit` crate, or raw `objc2` bindings
  (`objc2 = 0.6` is already a dependency). Audio via the same stream.
- Windows: the `windows-capture` crate or the `windows` crate (`Direct3D11CaptureFramePool`, WGC).
- Linux: `ashpd` (xdg-desktop-portal ScreenCast) plus the `pipewire` crate for the stream; PipeWire
  development headers are already installed in the Linux CI jobs.
- Record the decision as `docs/adr/0004-native-capture-backends.md` (existing ADRs are in `docs/adr/`).

---

## 5. Tasks

Conventions: **Depends** = task IDs that must pass their gate first. **Verify here** = what an agent
in a Linux sandbox can check. **Hardware** = needs a real machine and is `UNVERIFIED` until a human
confirms.

### NC-0 — Baseline and safety net (P0)
- **Depends:** none.
- **Steps:** tag the current working state `capture-ffmpeg-baseline`; add a backend switch
  `LOCUS_CAPTURE_BACKEND=ffmpeg|native` (default `ffmpeg` until native is proven per platform);
  keep `screen.rs` behaviour untouched and rename/move it only if a trait boundary requires it.
- **Acceptance:** with the switch at `ffmpeg`, all existing tests and the maintainer's Mac flow are
  unchanged. No behaviour change.
- **Evidence:** test counts before/after, `git diff --stat`.

### NC-1 — Shared capture layer (P0)
- **Depends:** NC-0. **Docs:** SPEC l.229 (common monotonic timeline), FR1.10/FR12.5, M2.03, M2.04.
- **Files:** new `capture/frame.rs`, `capture/clock.rs`, `capture/pacer.rs`,
  `capture/video_encoder.rs` (names indicative); small integration in `mod.rs`.
- **Steps:** implement `VideoFrame`, `MediaClock`, `FramePacer`, `ScreenSource` trait, a **synthetic
  frame source** for tests, and `VideoEncoder` that pipes raw frames to FFmpeg and rotates segments
  on pause. Keep the segment-per-pause + stop-both-at-once + resume-waits-for-video ordering.
- **Acceptance (all verifiable in Linux CI):**
  - Synthetic 30 fps source for N seconds → MP4 whose frame count is N×30 ±1 and duration within
    ±100 ms (no drift), including a static-screen run where the source emits almost no frames.
  - Pause 5 s mid-recording → no gap, duration excludes the pause (M2.04 gate).
  - Backpressure test: a deliberately slow FFmpeg consumer drops frames without blocking and without
    unbounded memory growth.
  - Strict-FFmpeg CI (`LOCUS_REQUIRE_FFMPEG=1`) runs these tests.
- **Hardware:** none.

### NC-2 — macOS ScreenCaptureKit video adapter (P0)
- **Depends:** NC-1. **Docs:** FR1.3, FR1.12, NFR10, M2.02, DESIGN l.390.
- **Files:** `capture/macos.rs` (adapter + preflight), `Cargo.toml` (macOS-only deps), `Info.plist`/
  `Entitlements.plist` if required.
- **Steps:** enumerate displays and windows; capture a display or a window (FR1.3); request NV12 and
  a capped output size; map frames to the shared clock; handle idle frames; permission preflight that
  triggers the OS prompt on first use (M2.02 gate) and a clear error if denied; handle display
  changes (see 4.2). Expose a **source list** and a **selected target** through the existing Tauri
  command layer; the picker UI itself is frontend-owned (Rule 4).
- **Acceptance:** records 30 s of screen on macOS 13+ with first frame well under the current ~2 s
  (**proposed target: ≤500 ms from Start to first frame; confirm with maintainer**); pause/resume
  without a 2 s wait; permission denial produces an actionable error and no phantom "recording".
- **Verify here:** `cargo check` for the macOS target in macOS CI; adapter logic behind the trait
  with unit tests using recorded/synthetic sample buffers where possible.
- **Hardware (UNVERIFIED until a Mac run):** actual capture, permission prompt, Retina scaling,
  multi-display, window capture, Tahoe-specific permission behaviour (macOS may periodically
  re-confirm screen-recording permission; **verify** and document).

### NC-3 — macOS audio on ScreenCaptureKit and microphone path (P0)
- **Depends:** NC-2. **Docs:** FR1.1, FR1.2, FR1.12, NFR10, F3.
- **Steps:** capture system audio from the same ScreenCaptureKit stream so it shares the video
  clock; keep the microphone on its own path (CoreAudio via `cpal`, or ScreenCaptureKit microphone
  where the OS allows it — **verify availability by OS version**); remove the 14.6 gate for the
  ScreenCaptureKit path and **keep the CoreAudio-tap path only as an explicit fallback** if desired.
  Preserve per-source start times and the existing aligned mix until NC-4 lands.
- **Acceptance:** system audio works on macOS 13.x (the PRD minimum); mic + system + video stay in
  sync (**proposed: within ±100 ms over a 10-minute run, confirmed by a visible/audible marker**);
  no regression of the pause/resume behaviour.
- **Hardware:** UNVERIFIED until run on macOS 13/14/15/26.

### NC-4 — Separate audio tracks in the output (P0, fixes F4)
- **Depends:** NC-1 (can be built against the FFmpeg backend first). **Docs:** FR1.13, FR12.2, FR1.11,
  M2.03, SPEC l.165.
- **Decision D2 first** (see §7). Recommended: final MP4 carries **up to three audio streams** —
  default = mixed (plays in any player, NFR12), plus `system` and `microphone` streams (non-default),
  each tagged by a stream title/disposition; the manifest lists each stream.
- **Steps:** encode per-source AAC (or encode from the per-source WAVs retained today), write the
  streams with metadata, expose which stream the diarization/transcription consumer should read
  (microphone stream feeds FR12.2). Audio-only sessions produce audio-only MP4 with **no dummy video**.
- **Acceptance:** `ffprobe` shows the expected streams and dispositions; audio-only MP4 has zero
  video streams; a mic-only fixture and a system-only fixture each decode independently; the sidecar
  contract (SPEC l.165) names the correct file/stream.
- **Verify here:** fully, with synthetic WAV inputs and `ffprobe`.

### NC-5 — Durable manifest and crash recovery (P0, fixes F5)
- **Depends:** NC-1, NC-4. **Docs:** FR12.3, FR12.4, SPEC l.228/230, M2.05, M2.06.
- **Steps:** wire `encoder.rs` (`Manifest`, `Segment`, `recoverable_manifests`) and the
  `capture_manifests` / `media_segments` tables into the manager: register each segment as it is
  completed; checkpoint so a process kill loses ≤5 s; make audio crash-safe (write recoverable
  segments or a recoverable container instead of a WAV whose header is only finalised on stop);
  on start-up detect interrupted captures and expose them (**never auto-restart capture**);
  keep raw segments until the final MP4 is verified, then delete. Stop-and-preserve on disk-full,
  forced sleep, permission revocation, source loss.
- **Acceptance (M2.05 gate):** force-kill during recording → relaunch offers recovery → recovered MP4
  loses ≤5 s and earlier segments are intact. Disk-full and source-disconnect tests stop and preserve.
- **Verify here:** kill tests with the synthetic source on Linux.
- **Note:** FR12.4 (window close keeps capture running; tray controls) is not covered by this review.
  Confirm its state with the maintainer and treat it as separate.

### NC-6 — Windows Graphics Capture + WASAPI (P0)
- **Depends:** NC-1 (NC-4 recommended first). **Docs:** FR1.12, NFR11, M10.11, DESIGN l.391, l.580.
- **Steps:** WGC video adapter (Windows 10 1903+), WASAPI loopback + microphone (verify whether
  `cpal`'s WASAPI backend meets sync/latency needs or whether a direct `windows`-crate WASAPI client
  is required), shared clock, display/window selection, permission/privacy behaviour. Decide
  the capture border/cursor behaviour (**verify per Windows version**). Replace the stub in
  `windows.rs`.
- **Acceptance:** records audio + screen on Windows 10 1903+; valid MP4; same sync/pause criteria as
  NC-2/3; Windows CI compiles and runs the shared-layer tests.
- **Hardware:** UNVERIFIED until a Windows run. Encoder fallback behaviour per D1.

### NC-7 — Linux PipeWire portal and audio (P0)
- **Depends:** NC-1 (NC-4 recommended first). **Docs:** FR1.14, NFR12a, M10.10, DESIGN l.392, l.579.
- **Steps:** xdg-desktop-portal ScreenCast session (permission dialog on first use, persisted
  restore token behaviour **to verify**), PipeWire video stream, PipeWire audio capture (D5), Wayland
  only (no X11/PulseAudio fallback per SPEC l.140). Replace `linux.rs` preflight accordingly.
- **Acceptance:** on Ubuntu 22.04+ with PipeWire, records system audio + screen via the portal and
  produces a valid MP4; clear errors when the portal or PipeWire is missing.
- **Hardware:** UNVERIFIED until run on a real Wayland desktop. A headless sandbox cannot show the
  portal dialog.

### NC-8 — Minimal FFmpeg build for encode/mux only (P0)
- **Depends:** D1 decided; NC-2 (so macOS no longer needs AVFoundation). **Docs:** SPEC l.141/336,
  DESIGN l.586/601, PRD risk row "Encoding/distribution compatibility".
- **Steps:** update `.github/workflows/build-ffmpeg.yml` and `packaging/ffmpeg-smoke.sh` to enable
  exactly what encoding needs (rawvideo input demux/decoder path as used, chosen H.264 encoder(s),
  `aac`, `mp4`/`concat`, `scale`/`format`/`fps` filters as used, the `setts` bitstream filter if kept)
  and **drop** `avfoundation`/`gdigrab`. Keep the smoke test identical to the production command
  line so a missing component fails in CI, not on a user's machine. Update
  `docs/release/THIRD_PARTY_NOTICES.md` and the licence statement for the chosen encoders.
- **Acceptance:** smoke test and `verify-bundle.sh` pass on all three installers; the notices list
  the exact build; macOS signing/notarisation of the nested binary is demonstrated.
- **Hardware/CI:** needs the GitHub runners (not reproducible in a sandbox).

### NC-9 — Validation and edge cases (P0 before declaring a platform supported)
- **Depends:** the platform's adapter task. **Docs:** SPEC §Capture and recovery, M2.06, M2.08.
- **Matrix to run and record (per platform):** first-frame latency; A/V sync marker test; pause/resume
  (including several pauses); sleep/wake; permission revoked mid-recording; selected display/window
  removed; disk nearly full; 3-hour synthetic run (bounded memory, ≤ expected size, warning at 3 h, no
  auto-stop — FR12.5); crash mid-recording (NC-5); mic-only, system-only, audio-only, all three.
- **Evidence:** fill the table in §8 per platform. Anything not run is `UNVERIFIED`.

### Dependency order

```
NC-0 → NC-1 → NC-4 ─┬→ NC-5
        │            └→ (NC-6, NC-7 can start after NC-4)
        └→ NC-2 → NC-3 ─→ NC-8 → NC-9 (per platform)
```

---

## 6. Edge cases and holes found in the documents

1. **SPEC says "reconcile all streams to a common monotonic media timeline" but does not define the
   clock origin or per-source start offsets.** Define: origin = moment the first source delivers its
   first sample after "recording" begins; record per-source offsets in the manifest.
2. **FR1.13 (separate tracks) vs the interim output (one mixed track)** — see F4. The PRD/IMPLEMENTATION
   language does not say whether separate tracks are extra MP4 streams or sidecar files (D2).
3. **FR1.3 "specific window"** has no corresponding UI or contract. Needs a Tauri command contract for
   listing/choosing sources before the frontend task can start.
4. **NFR10 (macOS 13+) conflicts with the interim 14.6 gate** (F3).
5. **FR12.4 (close keeps capture running, tray controls)** is not verified by this review; confirm.
6. **Frame pacing is unspecified.** ScreenCaptureKit sends no frames for an unchanging screen; the
   documents do not state the video frame rate. Proposed default: constant 30 fps (D4).
7. **Output resolution/bitrate are unspecified.** Retina sources are very large. Proposed: configurable
   cap (default ≤1080p) and CRF-based quality (D4).
8. **Encoder licensing/fallback** is a stated gate but undecided (D1).
9. **Window-title/privacy:** the documents say nothing about excluding notifications or specific
   windows from capture. Out of scope; note for the maintainer.
10. **Windows/Linux audio libraries** are not prescribed beyond "WASAPI" and "PipeWire"; the current
    code uses `cpal` for both (F9/F10).
11. **Storage paths** (F11) will break installed builds; unrelated to capture but blocks release QA.

---

## 7. Open decisions for the maintainer (the agent must not guess these)

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | H.264 encoder(s) | libx264 (GPL) / VideoToolbox / Media Foundation / OpenH264 / vendor hardware | Per-platform hardware encoder where available, with a distributable software fallback; make the GPL decision explicitly and update notices. |
| D2 | Where separate audio tracks live | extra MP4 audio streams (default = mixed) / sidecar files listed in the manifest | Extra MP4 streams with the mixed track as default; matches M2.03 wording and keeps one file per meeting. |
| D3 | Keep the FFmpeg-capture fallback after native is proven? | remove / keep behind flag | Keep behind the flag for one release, then remove. |
| D4 | Frame rate, resolution cap, quality | 24/30/60 fps; 1080p/1440p/native; CRF/bitrate | 30 fps, ≤1080p default, CRF-based. |
| D5 | Linux audio path | `cpal` (ALSA via PipeWire) / native PipeWire stream | Native PipeWire to match NFR12a; accept `cpal` only if the maintainer waives it. |
| D6 | macOS distribution | universal binary / per-arch | Decide with the FFmpeg build (NC-8): one binary per arch is simpler to sign. |
| D7 | Target latency and sync tolerances | proposed: ≤500 ms to first frame, ±100 ms sync | Confirm or replace; these are not in the documents. |

---

## 8. Evidence template (one per task)

```
Task: NC-n   Agent/model:   Date:   Commit:
Docs followed: (IDs)
Changes: (files, one line each)
Checks run (exact results):
  cargo fmt --check:
  cargo clippy --all-targets -D warnings:
  cargo test --lib (LOCUS_REQUIRE_FFMPEG=1):   N passed / M failed (pre-existing: ...)
  pnpm test / pnpm build:
  CI runs (links):
Hardware/OS checks:  PASS / FAIL / UNVERIFIED  — machine, OS version, FFmpeg version
Measured: first-frame latency, sync offset, frame-count error, recovery tail loss
Open issues / deviations from the documents:
```

---

## 9. Appendix — reference facts

### 9.1 Environment and versions seen
- Maintainer: macOS Tahoe 26.5.2, `pnpm tauri dev`, Homebrew FFmpeg 9.0.2 (dynamic, with libx264,
  libvmaf, videotoolbox).
- Sandbox: Ubuntu 24.04, Rust 1.91, FFmpeg 6.1.1 and an FFmpeg git-master build (Oct 2026) for
  comparison. No display, audio device, Mac or Windows machine.
- `cpal 0.18`, `hound 3.5`, `objc2 0.6`, Tauri 2.11 (`protocol-asset`).

### 9.2 Maintainer's ffprobe (interim build, before the timestamp fixes)
```
Duration: 00:00:21.06, start: 0.000000
Stream #0:0 Video: h264 (High), yuv420p, 1920x1080, 1091 kb/s, 30 fps, start 1.963997
Stream #0:1 Audio: aac (LC), 48000 Hz, mono, 163 kb/s
```
Video started ~2 s after audio (F1) and only one audio stream existed (F4). Interim fixes (not yet
re-measured by the maintainer at the time of writing): `setpts=PTS-STARTPTS` at encode,
`setts=ts=TS-STARTDTS` at mux, and a `sync-report.txt` next to each recording.

### 9.3 Interim files and flags worth knowing
- Outputs per meeting: `system_audio.wav`, `microphone.wav`, `audio.wav` (aligned mono mix),
  `recording.mp4`, `sync-report.txt`; raw `screen-NNN.mp4` segments are deleted after a successful mux
  unless `LOCUS_KEEP_SEGMENTS=1`.
- Env: `LOCUS_FFMPEG` (explicit FFmpeg path), `LOCUS_REQUIRE_FFMPEG=1` (CI: FFmpeg tests fail instead
  of skipping), `LOCUS_SCREEN_TEST_INPUT=lavfi` (synthetic screen source for lifecycle tests).
- Release builds look for a bundled FFmpeg only; debug builds also probe Homebrew paths and `PATH`.
  Bundle locations: macOS `Contents/Resources/ffmpeg/`, Windows `ffmpeg/` beside the exe, Linux
  `../lib/Locus/ffmpeg/` relative to `usr/bin` (untested against real installers).
- Packaging: `packaging/fetch-ffmpeg.sh` (pinned, SHA-256-verified download; refuses `TODO`
  hashes), `packaging/ffmpeg-smoke.sh`, `packaging/verify-bundle.sh`,
  `.github/workflows/build-ffmpeg.yml` (one-off static build), `.github/workflows/release.yml`.
  Neither workflow has been run on GitHub runners.

### 9.4 Commands
```sh
pnpm test && pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
LOCUS_REQUIRE_FFMPEG=1 cargo test --manifest-path src-tauri/Cargo.toml --lib
ffprobe -v error -show_entries stream=codec_type,start_time,duration -of csv=p=0 recording.mp4
```

### 9.5 Things the agent must not assume
- That any candidate crate in §4.3 works as described. None was built or run here.
- That ScreenCaptureKit, WGC or PipeWire behave identically across OS versions. Check by version.
- That a passing Linux CI run says anything about macOS/Windows capture behaviour.
- That the post-stop pipeline already consumes the recorded files (F8).

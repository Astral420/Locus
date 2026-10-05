# Native capture backends

Screen and system-audio capture move from a FFmpeg-driven device capture (about 2 s to the first frame, timestamps reconciled after the fact) to the operating system's own capture APIs feeding one shared media clock, with FFmpeg only encoding. macOS comes first through ScreenCaptureKit (NC-2 video, NC-3 system audio); Windows Graphics Capture and Linux PipeWire follow. The native path is selected with `LOCUS_CAPTURE_BACKEND=native`; the FFmpeg path stays the default and the fallback until each platform is proven on hardware.

## macOS library choice

Use the `screencapturekit` crate, pinned to 10.0.3 with only the `macos_13_0` feature (macOS 13 is the PRD minimum, NFR10). Checked 2026-10-03:

- Licence MIT OR Apache-2.0; published by doom-fish; about 2.4 million downloads; 11.0.0 released 2026-09-24.
- 10.0.3 needs Rust 1.76, within the repo's `rust-version = 1.77`. 11.0.0 needs Rust 1.82, and was nine days old, so it is not adopted yet.
- `SCStream` is `Send + Sync`; samples arrive through closure handlers; frame status distinguishes real frames from idle notices.
- Raw `objc2` bindings were the alternative; they would mean writing and maintaining the ScreenCaptureKit surface ourselves.
- The microphone is not taken from ScreenCaptureKit: that needs the `macos_15_0` feature and macOS 15. The microphone stays on `cpal`/CoreAudio.

## How it fits

`MacScreenSource` (`capture/macos_sck.rs`, macOS only) implements the shared `ScreenSource` trait. It asks for bi-planar 4:2:0 ("420v", NV12) at a fixed output size capped to 1080p, packs each frame tightly and stamps it on the shared `MediaClock`. Idle/blank notices carry no picture and are skipped; the frame pacer repeats the last frame to hold 30 fps. Pause only stops forwarding frames, so resume is instant. The output size is fixed for the whole recording, so a display switch or window resize is scaled by ScreenCaptureKit and never changes the encoded size.

System audio (`SystemAudioStream`) is a second ScreenCaptureKit stream (2x2 picture, never read) so audio-only sessions work. Its samples feed the same per-source WAV, level meter and pause handling as the `cpal` tracks, and its start time is the first buffer's arrival minus its length. With the native backend selected the 14.6 gate is replaced by a macOS 13 gate; the CoreAudio-tap path remains for `ffmpeg` mode.

Logic that needs no OS (size capping, plane packing, audio downmix, permission wording, version gates) is in `capture/macos.rs` and tested on every platform.

## Frontend contract (frontend-owned UI)

- `list_screen_sources` returns `ScreenSourceInfo[]`: `{ target, title, width, height }`. First call on a Mac shows the screen-recording permission prompt.
- `target` is `{"kind":"primary_display"}`, `{"kind":"display","id":<u32>}` or `{"kind":"window","id":<u32>}`.
- `start_recording` takes an optional `screenTarget` of that shape; omitted means the primary display. The FFmpeg backend ignores it.
- Permission denied surfaces as the start error, with the System Settings path in the message. macOS applies a new grant only after the app restarts.

## Output size and quality (proposals, not decisions)

30 fps, at most 1920x1080, CRF 26, `libx264` as in the interim path. Encoder choice and licensing (D1) and the numbers (D4, D7) remain maintainer decisions.

## Hardware checklist (UNVERIFIED until run on a Mac)

Written without a Mac to compile or run on. First `cargo check` and `cargo test --lib` on macOS, then with `LOCUS_CAPTURE_BACKEND=native`:

1. Permission: fresh install shows the prompt; deny gives the actionable error and no phantom "recording".
2. 30 s of screen: first frame well under 2 s (proposed target 500 ms); MP4 plays; frame count about 30 x duration.
3. Pause and resume: no 2 s wait; no gap; duration excludes the pause.
4. Retina and an external display; a window target; a window resized or closed mid-recording; display unplugged (expect a stopped capture that keeps the take).
5. System audio on macOS 13.x, 14.x, 15.x and 26: audio present; mic + system + video in sync (proposed: within 100 ms over 10 minutes, with a clap or flash marker).
6. Screen permission re-confirmation on newer macOS: note what appears and when.
7. DRM or protected content may record black; that is expected.
8. Colour: frames are tagged as video range ("420v"); check colours against the screen and, if they look washed out or shifted, set the colour range and matrix explicitly in the encoder.
9. Whether a silent system delivers audio buffers at all, since the audio start time uses the first buffer.

## Crash recovery (NC-5)

While recording, the take is kept rebuildable on disk:

- `capture-manifest.json` is written when capture starts: sources, whether there is video, the audio-vs-video offset, each audio track's lead, and (as pauses close segments) the completed video segments. The same segments are registered in `media_segments`.
- Each source's WAV has its header and buffered samples refreshed every second, so a killed process leaves a readable file. Recovery also repairs a header from the file's real length.
- Video segments are fragmented MP4 (a fragment per second), readable after a kill.

On a normal stop the final MP4 is built, read back and verified (clean packet read, expected streams, non-zero duration) before the raw video segments and derived aligned WAVs are deleted. The raw per-source WAVs and `audio.wav` are kept, because the transcription pipeline may read them. `LOCUS_KEEP_SEGMENTS=1` keeps everything.

A watchdog runs every 2 s. If the screen source died (display unplugged, window closed, permission revoked) or an audio write failed (disk full), the capture is stopped and finalised as far as possible and left in the "recoverable" state with the reason. A finalised, verified recording is marked saved so recovery never rebuilds over it; otherwise the raw files stay and recovery can rebuild later. A preserved capture no longer blocks starting a new one.

On the next launch, `list_recoverable_captures` offers interrupted captures (it flips orphans from "recording" to "interrupted"); `recover_capture(meetingId)` rebuilds the recording, saves it like a normal stop and queues processing. Nothing is restarted automatically.

Frontend contract: `list_recoverable_captures` returns `{ meetingId, captureId, sources, hasVideo, videoSegments, audioFiles, estimatedSeconds, reason }[]`; `recover_capture` returns `{ meetingId, relativePath, durationSeconds, hasVideo, audioStreams }`. Deciding when to show the offer is the frontend's.

Not covered, and not verified: forced sleep and power loss (data reaches the OS cache but not necessarily the platter; there is no fsync on audio), network drives, and a screen-source loss on real hardware (the macOS stop callback is untested on a Mac). A discard command for unwanted interrupted captures does not exist yet.

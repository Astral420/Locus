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

## Windows (NC-6)

Library: `windows-capture` 2.0.1 (MIT, 1.9 million downloads, updated 2026-08-08), a wrapper over Windows Graphics Capture that owns the Direct3D device, frame pool and staging copy. The raw `windows` crate was the alternative; it would mean writing and maintaining all of that ourselves with no Windows machine to test on. Two costs: the crate is Rust edition 2024 (needs Rust 1.85; the repo's declared `rust-version = 1.77` should be raised), and `Frame::buffer()` allocates a staging texture per frame (acceptable at 30 fps; reuse it if profiling says so).

`WinScreenSource` (`capture/windows_wgc.rs`, Windows only) implements the shared `ScreenSource` trait:

- Targets: `Display(n)` is the one-based monitor index; `Window(id)` is the window's `HWND`; primary display by default. `list_screen_sources` returns them with titles.
- Pixel format: WGC delivers BGRA, so frames go to FFmpeg as BGRA and FFmpeg converts to 4:2:0. That is about 250 MB/s through the pipe at 1080p30. A source larger than 1080p (a 4K display is 1 GB/s of BGRA) is first averaged down in Rust to the fixed output size (`scale_bgra`, area average, sharp on text), so the pipe never carries more than a 1080p frame. GPU-side NV12 conversion is the follow-up if CPU use matters.
- Cursor and border: the OS defaults. That is the only combination valid on every build from Windows 10 1903; the cursor is recorded and Windows draws its capture border. Removing the border needs Windows 11 and a permission, so it is deliberately not requested.
- Pause only stops forwarding pictures, as on macOS. The window or display closing ends the stream and the watchdog preserves the take.
- Preflight rejects Windows builds below 18362 (version 1903) with a clear message when the native backend is selected.

Audio: not changed. System audio already runs through `cpal`'s WASAPI loopback (an input stream on the default output device) and the microphone through its WASAPI input, both timestamped on the shared media clock at the moment of their first buffer. Whether `cpal` meets the sync and latency targets on Windows is **unverified**; move to a direct `windows`-crate WASAPI client only if the checklist below shows drift or excess latency.

Hardware checklist (UNVERIFIED until run on Windows 10 1903+ and Windows 11):

1. `cargo check` and `cargo test --lib` on Windows; then record with `LOCUS_CAPTURE_BACKEND=native`.
2. Primary display, a second monitor, a window; window resized and closed mid-recording; display unplugged.
3. First frame time; frame count about 30 x duration; 4K display stays under one frame of CPU per frame.
4. Pause and resume: no gap, duration excludes the pause.
5. System audio (loopback) and microphone present and in sync with video within the D7 tolerance over 10 minutes; behaviour when the default output device changes mid-recording.
6. The yellow capture border on 10 and 11; cursor visible; a protected (DRM) window records black.
7. Behaviour on a GPU-less or remote-desktop session.
8. Windows CI runs the shared-layer tests: `cargo test --lib` on a Windows runner needs FFmpeg on `PATH` or `LOCUS_REQUIRE_FFMPEG` unset.

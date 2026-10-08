# Native capture backends

Screen and system-audio capture move from a FFmpeg-driven device capture (about 2 s to the first frame, timestamps reconciled after the fact) to the operating system's own capture APIs feeding one shared media clock, with FFmpeg only encoding. macOS comes first through ScreenCaptureKit (NC-2 video, NC-3 system audio); Windows Graphics Capture and Linux PipeWire follow. Each OS now defaults to its native path (`CaptureBackend::platform_default`: ScreenCaptureKit on macOS, Windows Graphics Capture on Windows, the PipeWire portal on Linux), with FFmpeg only encoding. `LOCUS_CAPTURE_BACKEND=ffmpeg` is an explicit override that forces the legacy FFmpeg-driven screen capture; `native` is accepted and means the same as the default. Hardware proof per platform is still outstanding (see each checklist).

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

Written without a Mac to compile or run on. First `cargo check` and `cargo test --lib` on macOS, then record with the default (native) backend:

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

1. `cargo check` and `cargo test --lib` on Windows; then record with the default (native) backend.
2. Primary display, a second monitor, a window; window resized and closed mid-recording; display unplugged.
3. First frame time; frame count about 30 x duration; 4K display stays under one frame of CPU per frame.
4. Pause and resume: no gap, duration excludes the pause.
5. System audio (loopback) and microphone present and in sync with video within the D7 tolerance over 10 minutes; behaviour when the default output device changes mid-recording.
6. The yellow capture border on 10 and 11; cursor visible; a protected (DRM) window records black.
7. Behaviour on a GPU-less or remote-desktop session.
8. Windows CI runs the shared-layer tests: `cargo test --lib` on a Windows runner needs FFmpeg on `PATH` or `LOCUS_REQUIRE_FFMPEG` unset.

## Linux (NC-7)

Libraries: `ashpd` 0.13 (MIT) for the xdg-desktop-portal ScreenCast session and `pipewire` 0.10 (MIT, links the system `libpipewire-0.3`). `ashpd` runs on `async-io` (already in the lockfile) so no second async runtime is added; `ashpd` needs Rust 1.87, so `rust-version` is raised to 1.87. The CI Linux job already installs `libpipewire-0.3-dev`; building against the 0.3.48 shipped by Ubuntu 22.04 is **unverified**.

`LinuxScreenSource` (`capture/linux_pipewire.rs`, Linux only) implements the shared `ScreenSource` trait:

- Wayland does not let an app enumerate screens or windows. `list_screen_sources` returns one entry, "Screen or window (choose in the system dialog)"; the compositor's own chooser appears when recording starts. A `Display` target narrows the dialog to monitors, a `Window` target to windows; their ids are not used.
- No restore token is kept, so the dialog appears on every recording and a recording never starts on a screen the person did not just choose. Persisting a token needs a place to store it; follow-up if the repeated prompt is a problem.
- Buffers: only shared-memory 32-bit RGB (`BGRx`, `BGRA`, `RGBx`, `RGBA`) is offered, no DMA-BUF modifiers, so frames are read directly and reach FFmpeg as BGRA (RGB-order sources are swapped in Rust). Sources above 1080p are area-averaged down in Rust (`scale_bgra`), as on Windows; the output size is fixed from the first negotiated size.
- Cursor: embedded in the picture when the portal supports it.
- Pause only stops forwarding pictures. The share ending (stopped from the desktop, window closed, PipeWire gone) leaves the streaming state and `failure()` reports it so the watchdog preserves the take. No picture within 10 s of connecting is reported as an error (some compositors send only on damage).
- Preflight (native or not): X11 sessions are rejected; PipeWire must be reachable (a real connection, not `pw-cli`); the ScreenCast portal is checked only when the screen is a selected source.

Audio (`capture/linux_audio.rs`), decision D5 taken as the plan recommends: with the native backend (the default on Linux) system audio is a PipeWire capture stream on the default output's monitor (`stream.capture.sink`) and the microphone is the default input, both requested as mono F32 at 48 kHz so PipeWire's adapter does the conversion. They feed the same WAV writer, meter, pause and failure handling as the `cpal` tracks; the first buffer's arrival minus its length is the start time. With the `ffmpeg` override audio still goes through `cpal`. If D5 is decided the other way, `audio.rs` selects `cpal` again by dropping one branch.

Audio starts while the portal dialog is open, so it leads the video; the existing offset trim in the mux handles that, at the cost of recording the seconds spent in the dialog and discarding them.

Tested without a desktop: `linux_audio.rs` and `linux_pipewire.rs` have tests, run only with `LOCUS_PIPEWIRE_DAEMON_TEST=1`, against a headless PipeWire + WirePlumber with null devices (`pipewire`, `wireplumber`, a session D-Bus, `pw-cli create-node ... support.null-audio-sink` for a sink and a virtual source, GStreamer `pipewiresink` as a stand-in compositor). They prove the stream code: a tone played to the default output is heard, the microphone stream delivers buffers and stops cleanly, a red picture arrives as BGRA for both BGRx and RGBx sources, a 2560x1440 source is scaled to 1920x1080, pause and resume gate frames, and a vanished source is reported. They do not exercise the portal dialog, a real compositor, or real devices. The video stream is connected with `DONT_RECONNECT`; without it a vanished source left the stream paused and unreported.

Hardware checklist (UNVERIFIED until run on a Wayland desktop):

1. GNOME (mutter), KDE (KWin) and a wlroots compositor with `xdg-desktop-portal-wlr`: dialog appears, monitor and window both work, cancel gives a clear error.
2. First frame time; frame count about 30 x duration; a static screen on wlroots still sends a first picture within 10 s.
3. Window resized and closed mid-recording; "stop sharing" from the desktop ends the take and keeps it.
4. System audio from a browser and a game; microphone; default output changed mid-recording; headphones unplugged.
5. "Stop sharing" from the desktop ends the take through the portal path (only the PipeWire-node-removal route is tested; a watcher on the portal session's `Closed` signal is not implemented). A/V sync within the D7 tolerance over 10 minutes; first-buffer start time on a silent system (the audio start uses the first buffer; if a silent graph delivers none, the stream's "streaming" time is used after 5 s).
6. A 4K display; fractional scaling; a rotated monitor (stride and size come from the negotiated format).
7. Ubuntu 22.04 (PipeWire 0.3.48) and a current distribution.
8. Sandboxed (Flatpak/Snap) runs: the portal is the only route, but the PipeWire socket path differs.

## Bundled FFmpeg (NC-8)

FFmpeg no longer captures anything in the native design, so the bundled build (`.github/workflows/build-ffmpeg.yml`) drops `avfoundation`, `gdigrab`, `lavfi` and the test sources. The component list is what the production command lines need, and it was checked by building FFmpeg 7.1.1 with exactly these flags and running `packaging/ffmpeg-smoke.sh` against the result (the workflow itself has not run on a CI runner):

- encoders `libx264`, `aac`, plus `wrapped_avframe` and `pcm_s16le` (the `null` muxer used by the full-decode verification needs both);
- decoders `h264`, `aac`, `rawvideo`, `pcm_s16le`;
- demuxers `rawvideo` (frames on a pipe), `mov`, `concat`, `wav`; muxers `mp4`, `null`; protocols `file`, `pipe`;
- filters `scale`, `format`, `fps`, `aresample`, `aformat`, `anull`, `null`;
- parsers `h264`, `aac`, and `hevc`: FFmpeg 7.1.1's `h2645_sei.c`, built for the H.264 decoder, calls AOM film-grain code that only an HEVC component builds, so an H.264-only build fails to link without it (HEVC header parsing only, no decoder);
- bitstream filters `setts` (segment re-time) and `h264_mp4toannexb` (the concat demuxer refuses H.264 without it).

The list is sufficient, not proven minimal: items were added when a smoke command failed, never removed one by one. The smoke test feeds raw NV12 and BGRA frames on a pipe, re-times the segment, concatenates two segments with three AAC streams (positive and negative audio offsets), builds an audio-only file, and fully decodes each result.

Because native capture is now the default on every supported OS, a default build needs no capture device. The workflow keeps a `capture_devices` input (default off) that adds `avfoundation`/`gdigrab` back; build with it only while the `LOCUS_CAPTURE_BACKEND=ffmpeg` override must work in a shipped build (plan D3). On Linux the override has never been able to record the screen.

The encoder is `libx264` (GPL; decision D1 confirmed). If it ever changes, `--enable-encoder`, the smoke test and `THIRD_PARTY_NOTICES.md` change together.

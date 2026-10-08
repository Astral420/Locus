# Bundled FFmpeg

Locus encodes and finalizes `recording.mp4` with FFmpeg. Screen and audio capture
are native (ScreenCaptureKit, Windows Graphics Capture, PipeWire); FFmpeg only
receives raw frames and WAV files. This folder
is listed in `tauri.conf.json` → `bundle.resources`, so whatever is placed here
ships inside the installer (PRD FR1.11, DESIGN "bundled binary").

Place one **static, self-contained** build per platform before running
`pnpm tauri build` (the binary itself is git-ignored):

| Platform | File | Notes |
|---|---|---|
| macOS (arm64 / x86_64) | `ffmpeg/ffmpeg` | Build each arch (or a universal binary). Must be codesigned with the app's Developer ID and included in notarization. Homebrew's FFmpeg is dynamically linked and will **not** work when copied. |
| Windows x64 | `ffmpeg/ffmpeg.exe` | Static build. |
| Linux x86_64 | `ffmpeg/ffmpeg` | Static build. |

Required capabilities: `libx264`, `aac`, `wrapped_avframe` and `pcm_s16le`
encoders; `h264`, `aac`, `rawvideo`, `pcm_s16le` decoders; `rawvideo`/`mov`/
`concat`/`wav` demuxers; `mp4`/`null` muxers; `pipe`/`file` protocols;
`scale`/`format`/`fps`/`aresample`/`aformat` filters; `h264`/`aac`/`hevc`
parsers (`hevc` works around an FFmpeg 7.1.1 link bug); `setts` and
`h264_mp4toannexb` bitstream filters. The exact list is the
`configure` line in `.github/workflows/build-ffmpeg.yml`, checked by
`packaging/ffmpeg-smoke.sh`. No capture device is needed: each OS captures
natively by default. Only the `LOCUS_CAPTURE_BACKEND=ffmpeg` override needs
them (build with `capture_devices: true`).

**License gate:** `libx264` makes the build GPL. Audit the exact build against
https://ffmpeg.org/legal.html and add it to `docs/release/THIRD_PARTY_NOTICES.md`,
or switch the encoder to an LGPL-compatible path (`h264_videotoolbox` on macOS,
`h264_mf` on Windows, OpenH264) in `src/capture/screen.rs`.

Development: set `LOCUS_FFMPEG=/path/to/ffmpeg`, drop a binary here, or (debug
builds only) rely on a system FFmpeg on PATH. Release builds never use a system
FFmpeg.

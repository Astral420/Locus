# Bundled FFmpeg

Locus records the screen and finalizes `recording.mp4` with FFmpeg. This folder
is listed in `tauri.conf.json` → `bundle.resources`, so whatever is placed here
ships inside the installer (PRD FR1.11, DESIGN "bundled binary").

Place one **static, self-contained** build per platform before running
`pnpm tauri build` (the binary itself is git-ignored):

| Platform | File | Notes |
|---|---|---|
| macOS (arm64 / x86_64) | `ffmpeg/ffmpeg` | Build each arch (or a universal binary). Must be codesigned with the app's Developer ID and included in notarization. Homebrew's FFmpeg is dynamically linked and will **not** work when copied. |
| Windows x64 | `ffmpeg/ffmpeg.exe` | Static build. |
| Linux x86_64 | `ffmpeg/ffmpeg` | Static build. |

Required capabilities: `avfoundation` (macOS) / `gdigrab` (Windows) input,
`libx264` + `aac` encoders, `mp4`/`concat` muxers, `scale`/`format` filters.

**License gate:** `libx264` makes the build GPL. Audit the exact build against
https://ffmpeg.org/legal.html and add it to `docs/release/THIRD_PARTY_NOTICES.md`,
or switch the encoder to an LGPL-compatible path (`h264_videotoolbox` on macOS,
`h264_mf` on Windows, OpenH264) in `src/capture/screen.rs`.

Development: set `LOCUS_FFMPEG=/path/to/ffmpeg`, drop a binary here, or (debug
builds only) rely on a system FFmpeg on PATH. Release builds never use a system
FFmpeg.

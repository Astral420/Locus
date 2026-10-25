# Locus fixes — how to apply

Already committed an earlier version? Just overwrite and review the diff:

    unzip -o locus-fixes.zip
    cp -R locus-fixes/. . && rm -rf locus-fixes
    chmod +x packaging/*.sh
    git diff --stat        # only the newest changes show up

Then:  pnpm test && cargo test --manifest-path src-tauri/Cargo.toml --lib

Local dev: the bundled FFmpeg is intentionally minimal, so the FFmpeg-backed
`capture::*` tests skip it. Point them at a full-featured FFmpeg (with
`ffprobe` on `PATH`) instead of overriding the app's bundled binary:

    LOCUS_TEST_FFMPEG=$(which ffmpeg) cargo test --manifest-path src-tauri/Cargo.toml --lib

To make the app itself use a system FFmpeg during development, set
`LOCUS_FFMPEG` instead (debug builds also find one on `PATH`).

## Debugging A/V sync
Every recording with video now gets  <media>/<meeting id>/sync-report.txt
(audio offset used + each stream's start time). Send that file if sync is off.
To keep the raw video pieces too:  LOCUS_KEEP_SEGMENTS=1 pnpm tauri dev

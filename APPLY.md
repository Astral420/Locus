# Locus fixes — how to apply

Already committed an earlier version? Just overwrite and review the diff:

    unzip -o locus-fixes.zip
    cp -R locus-fixes/. . && rm -rf locus-fixes
    chmod +x packaging/*.sh
    git diff --stat        # only the newest changes show up

Then:  pnpm test && cargo test --manifest-path src-tauri/Cargo.toml --lib
Local dev: export LOCUS_FFMPEG=$(which ffmpeg)

## Debugging A/V sync
Every recording with video now gets  <media>/<meeting id>/sync-report.txt
(audio offset used + each stream's start time). Send that file if sync is off.
To keep the raw video pieces too:  LOCUS_KEEP_SEGMENTS=1 pnpm tauri dev

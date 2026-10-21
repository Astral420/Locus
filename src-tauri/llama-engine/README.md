# Bundled llama-server engine

This folder is listed in `tauri.conf.json` → `bundle.resources`, so whatever is
placed here ships inside the app. It is filled by CI on **macOS x86_64 only**:
`packaging/llama-engine/build.sh` produces `packaging/llama-engine/dist/` and
`packaging/llama-engine/stage.sh` copies it here, verifies `SHA256SUMS`, checks
`engine.json`, and signs `llama-server`.

Keep the engine together: `llama-server` finds `kernels/` next to itself.
Everything except this README is git-ignored. Apple Silicon, Windows and Linux
builds ship no engine yet and keep using `llama-server` from `PATH`.

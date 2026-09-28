# Locus

Locus is an offline-first Tauri desktop meeting library. The M1–M3 foundation
now includes the Rust/React monorepo, a worker-thread SQLite schema, OS-keychain
wrapper, bounded JSON-RPC sidecar protocol, capture lifecycle and durable
manifests, Python sidecar methods for VAD/diarization/slides/OCR/vector storage,
and CI checks.

## Development checks

```sh
pnpm install --frozen-lockfile
pnpm build
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
python3 -m pytest sidecar/tests
```

The sidecar uses the locally provided `LOCUS_PYANNOTE_MODEL` for packaged
pyannote inference. When ML dependencies are absent, the test-only development
path uses deterministic stdlib fallbacks and never downloads a model. Native
ScreenCaptureKit, Windows Graphics Capture, and PipeWire adapters are explicit
platform seams and report unavailable until their platform-specific capture
bridges are qualified.

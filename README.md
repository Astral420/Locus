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

## Running from source: sidecar setup

`pnpm tauri dev` starts the Python sidecar (slide detection, slide text recognition, speaker
detection, search) with your own Python, so it needs its packages once:

```sh
scripts/setup-sidecar.sh        # creates .venv, installs the packages, downloads the speaker models
pnpm tauri dev                  # Locus finds .venv by itself; nothing to activate
```

Without it the app still records and transcribes; the optional steps are skipped with a plain message
("Slide text wasn't read because text recognition isn't installed on this computer.") and you can
press **Re-run** on them after setup. To use a different interpreter set `LOCUS_PYTHON`.

What is used, and why:

| Feature | Engine | Where its files come from |
| --- | --- | --- |
| Slide text | RapidOCR (pip; models are inside the wheel) | `scripts/setup-sidecar.sh` |
| Speaker labels | sherpa-onnx + pyannote segmentation 3.0 + a speaker-embedding model (ONNX, no PyTorch) | `scripts/fetch-diarization-models.sh` -> `models/diarization/` |
| Optional alternatives | Tesseract (`LOCUS_TESSERACT`), pyannote.audio (`LOCUS_PYANNOTE_MODEL`) | `sidecar/requirements-optional.txt` |

Model files are found through `LOCUS_MODEL_DIRS` (set by the app: your app-data `models/` folder, then
the bundled one), then the repository `models/` folder. The packaged sidecar (`sidecar/build.spec`) bundles
these engines and models, so an installed Locus needs no Python, Tesseract or PyTorch on the user's machine.
Before shipping, check the licence of the speaker-embedding model that `fetch-diarization-models.sh` downloads.

Native ScreenCaptureKit, Windows Graphics Capture, and PipeWire adapters are explicit platform seams and
report unavailable until their platform-specific capture bridges are qualified.

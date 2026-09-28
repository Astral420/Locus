# Locus release qualification evidence

This checklist is the release gate for M10.13 and is intentionally evidence-based. A cell is `pass` only with an attached log, artifact checksum, or screen recording. Hardware that is unavailable to CI stays `unverified`; it is never reported as passing.

| Gate | macOS Apple Silicon | macOS Intel + AMD | Linux Ubuntu 22.04 PipeWire | Windows 10/11 WGC + WASAPI |
|---|---|---|---|---|
| Clean install | unverified | unverified | unverified | unverified |
| Offline first launch / bundled capture | unverified | unverified | unverified | unverified |
| Audio and screen capture | unverified | unverified | unverified | unverified |
| CPU transcription baseline | CI | CI | CI | CI |
| Qualified GPU path | Metal: unverified | Vulkan/MoltenVK: unverified | CUDA/ROCm: unverified | CUDA/ROCm: unverified |
| Playback and transcript seek | unverified | unverified | unverified | unverified |
| Upgrade / verified manual package | unverified | unverified | unverified | unverified |
| Three-hour fixture | unverified | unverified | unverified | unverified |
| Signing / notarization / installer | unverified | unverified | unverified | unverified |

## Bundled assets

Record exact versions and SHA-256 digests before publishing:

- FFmpeg build and enabled codecs: `unverified`
- Whisper small model: `unverified` (see `models/README.md`)
- pyannote model and license: `unverified` (see `models/README.md`)
- Tesseract executable and language data: `unverified`
- PyInstaller sidecar: `unverified`
- llama-server and generation/embedding provisioning path: `unverified`
- MoltenVK dylib for the Intel Vulkan variant: `unverified`

## Release operator record

- Release version:
- Commit:
- Build host/toolchain:
- Artifact names and SHA-256:
- Reviewer:
- Date:
- Exceptions approved by maintainer:

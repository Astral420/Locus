# Third-party notices

Locus application code is MIT-licensed. The release process must replace each `unverified` entry below with the exact source revision, license text or notice, and packaged artifact checksum.

| Component | Use | License / source evidence | Bundled in release |
|---|---|---|---|
| Tauri / wry | Desktop shell | `pnpm-lock.yaml`, Cargo.lock | yes |
| FFmpeg 7.1.1 + x264 | MP4 encoding and muxing only (no capture devices) | GPL v2 or later (built with `--enable-gpl --enable-libx264`; also builds the `hevc` parser, parsing only, no HEVC decoder); recipe `.github/workflows/build-ffmpeg.yml`; exact FFmpeg and x264 revisions are in `BUILDINFO.txt` inside each archive. Source for both, plus the recipe, must be offered with the installer. | unverified: no release build has been run |
| ashpd / pipewire (Rust) | Linux screen portal and PipeWire streams | MIT; `Cargo.lock`. Links the system `libpipewire-0.3` (MIT) | Linux builds |
| whisper.cpp / whisper-rs | Transcription | exact commit and license required | unverified |
| pyannote community model | VAD and diarization | exact model revision and model license required | unverified |
| RapidOCR + PP-OCR models (ONNX) | OCR | RapidOCR is Apache-2.0 and its models come from PaddleOCR (Apache-2.0); exact package version, model files and notices required | unverified |
| llama.cpp / ggml | Local generation and embeddings, built into the patched engine below | MIT; commit `d81235049384534c167caea52b85a694f6103d14` of https://github.com/ggml-org/llama.cpp, pinned in `packaging/llama-engine/pin.env` | macOS x86_64, inside the patched engine |
| ToshLLM llama.cpp patch series (bundled `llama-server`) | AMD Metal fixes applied to llama.cpp; shipped as a separate `llama-server` binary that Locus starts as its own process over loopback HTTP and links nothing from | GPL-3.0-or-later; ToshLLM `v0.87.26`, commit `523b814ac8cc61d80afbc73cece581c34234658b` (https://github.com/engeldlgado/toshllm), notices in `LICENSE` and `patches/COPYRIGHT`. Corresponding source: `llama-engine-source-v0.87.26.tar.gz`, built by `packaging/llama-engine/source-offer.sh` and attached to each release next to the installer; see [ADR 0008](../adr/0008-llama-engine-license.md) | macOS x86_64 |
| MoltenVK | Intel Mac Vulkan translation | Apache-2.0; exact SDK version required | unverified |

Do not publish an installer while an asset required for offline first launch lacks a verified provenance record.

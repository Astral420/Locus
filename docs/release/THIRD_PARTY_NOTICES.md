# Third-party notices

Locus application code is MIT-licensed. The release process must replace each `unverified` entry below with the exact source revision, license text or notice, and packaged artifact checksum.

| Component | Use | License / source evidence | Bundled in release |
|---|---|---|---|
| Tauri / wry | Desktop shell | `pnpm-lock.yaml`, Cargo.lock | yes |
| FFmpeg 7.1.1 + x264 | MP4 encoding and muxing only (no capture devices) | GPL v2 or later (built with `--enable-gpl --enable-libx264`; also builds the `hevc` parser, parsing only, no HEVC decoder); recipe `.github/workflows/build-ffmpeg.yml`; exact FFmpeg and x264 revisions are in `BUILDINFO.txt` inside each archive. Source for both, plus the recipe, must be offered with the installer. | unverified: no release build has been run |
| ashpd / pipewire (Rust) | Linux screen portal and PipeWire streams | MIT; `Cargo.lock`. Links the system `libpipewire-0.3` (MIT) | Linux builds |
| whisper.cpp / whisper-rs | Transcription | exact commit and license required | unverified |
| pyannote community model | VAD and diarization | exact model revision and model license required | unverified |
| Tesseract | OCR | exact binary and traineddata notices required | unverified |
| llama.cpp / llama-server | Local generation and embeddings | exact commit and license required | unverified |
| MoltenVK | Intel Mac Vulkan translation | Apache-2.0; exact SDK version required | unverified |

Do not publish an installer while an asset required for offline first launch lacks a verified provenance record.

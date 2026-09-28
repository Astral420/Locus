# Third-party notices

Locus application code is MIT-licensed. The release process must replace each `unverified` entry below with the exact source revision, license text or notice, and packaged artifact checksum.

| Component | Use | License / source evidence | Bundled in release |
|---|---|---|---|
| Tauri / wry | Desktop shell | `pnpm-lock.yaml`, Cargo.lock | yes |
| FFmpeg | MP4 encoding and media processing | exact build recipe required | unverified |
| whisper.cpp / whisper-rs | Transcription | exact commit and license required | unverified |
| pyannote community model | VAD and diarization | exact model revision and model license required | unverified |
| Tesseract | OCR | exact binary and traineddata notices required | unverified |
| llama.cpp / llama-server | Local generation and embeddings | exact commit and license required | unverified |
| MoltenVK | Intel Mac Vulkan translation | Apache-2.0; exact SDK version required | unverified |

Do not publish an installer while an asset required for offline first launch lacks a verified provenance record.

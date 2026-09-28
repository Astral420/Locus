# Intel Mac + AMD Vulkan/MoltenVK qualification

M11 is a qualified-build path, not a promise that any Vulkan-enabled binary is safe to ship. The CPU transcription and local-generation paths remain the correctness baseline.

## Runtime contract

- Target architecture: `x86_64-apple-darwin` with a discrete AMD Radeon GPU.
- Build both whisper.cpp and llama.cpp from pinned reviewed commits with `GGML_VULKAN=1` and the pinned MoltenVK SDK.
- Set `GGML_VK_DISABLE_F16=1` and select the AMD device with `GGML_VK_VISIBLE_DEVICES=0` (the runtime may override the index after enumerating devices).
- Launch llama-server with `--n-gpu-layers -1 --flash-attn 0` on this path. Flash Attention is disabled because MoltenVK translation has produced corrupt output on AMD devices.
- Apple Silicon builds remain Metal-only. Intel Macs without a validated AMD dGPU use CPU fallback.

## Reproducible build record

The release operator must fill these fields from the build output:

| Artifact | Commit / SDK | Build flags | SHA-256 |
|---|---|---|---|
| whisper.cpp Vulkan binary | `unverified` | `-DGGML_VULKAN=1` | `unverified` |
| llama-server Vulkan binary | `unverified` | `-DGGML_VULKAN=1` | `unverified` |
| MoltenVK dylib | `unverified` | pinned SDK | `unverified` |

## Qualification fixtures

Record the exact machine, macOS version, driver/runtime, model file, and wall-clock measurements for:

1. Five-minute English transcription against the CPU baseline: word accuracy must be at least 80% of the baseline result.
2. 60-minute and three-hour transcription fixtures on at least two AMD dGPU models.
3. llama-server completion and embedding smoke tests with no gibberish or NaN vectors.
4. Model sizes within each GPU's VRAM budget and a clean-install Intel DMG test.

Until those measurements are attached, the M11 hardware gates remain `unverified`.

# Reproducible Vulkan build inputs

This directory describes the Intel macOS variant without vendoring unreviewed third-party binaries. Pin exact upstream commits, the MoltenVK SDK archive, and checksums in the release evidence before producing a DMG.

Required build definitions:

```sh
cmake -S whisper.cpp -B build/whisper-vulkan -DGGML_VULKAN=1 -DCMAKE_BUILD_TYPE=Release
cmake -S llama.cpp -B build/llama-vulkan -DGGML_VULKAN=1 -DCMAKE_BUILD_TYPE=Release
```

The packaging job must copy the resulting binaries and MoltenVK dylib into the Intel bundle, run `codesign --verify`, and write SHA-256 entries into `docs/release/VULKAN_MACOS_INTEL.md`. Floating fork release URLs are not accepted as build inputs.

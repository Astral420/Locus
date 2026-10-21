# llama-server engine (macOS x86_64 + AMD)

Builds the pinned ToshLLM-patched `llama-server` and precompiled Metal kernels for
document 02 to bundle. Nothing here runs at app runtime.

```sh
packaging/llama-engine/build.sh            # build (idempotent; --clean resets CMake)
packaging/llama-engine/build.sh --dry-run  # print pins and steps only
packaging/llama-engine/test.sh             # engine suites; no model or GPU
```

Output lands in `packaging/llama-engine/dist/`; ToshLLM is cloned into `.work/toshllm`.
Both directories are git-ignored. An Intel Mac builds and tests natively; an arm64 Mac
cross-compiles and runs the tests through Rosetta. The build fails on a HEAD mismatch.

To bump the pin: edit `pin.env`, run `build.sh`, then `test.sh`.

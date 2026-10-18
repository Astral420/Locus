# Model selection names an exact quantization, not a bundled model

ADR 0005 replaced the bundled whisper model with a verified catalog download. The Model Manager still reported one status per model and stored a `whisper:<size>:bundled` selection, so two installed quantizations could not be told apart and the transcriber could load the wrong file (`ggml-small-q5_1.bin` beat `ggml-small.bin`; `small` also matched `ggml-small.en.bin`). Selection now names a variant.

## Rules

- **Whisper selection value:** `whisper:<selector>:<variant>` (`whisper:small:standard`, `whisper:small.en:q5_1`). Legacy values (`whisper:small:bundled`, `whisper:tiny:catalog`) still resolve. LLM/embedding selection stays the asset id (`generation:{catalog_id}:{variant}-{commit8}`).
- **Whisper resolver grammar:** env override → exact file when the third segment is `standard` or `^q\d_\d$` → strict family match (`ggml-{selector}.bin` or `ggml-{selector}-q\d_\d.bin`, never `.en`/turbo leakage) → the old defaults. See `paths.rs::find_whisper_model`.
- **One status vocabulary:** `available`, `downloading`, `installed`, `active`, `error`. A neutral **Default** tag is separate from status. `bundled` is gone because provenance (file origin) is not UI state; the SQL `origin` check and `ModelOrigin::Bundled` still describe where a file came from.
- **Auto-activation (A1):** a successful download becomes the active model only if the role had no usable model before it (`had_model`); a later download never switches the active model. The `done` event is emitted after the selection is written so the UI refetch sees it.

Related: [0005](0005-model-catalog-and-first-run-download.md).

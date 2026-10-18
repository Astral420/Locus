# Uninstall models by deleting only Locus-managed files

Users can free disk space without leaving Locus: remove one quantization or every quantization of a model. The
Model Manager shows `removable` only for files inside the folders Locus owns (`whisper_models()`,
`embedding_models()`, `generation_models()`); a whisper file found in `paths.models` or `bundled_models` is
installed but never deletable, and no code path deletes a directory or a file outside those folders.

## Rules

- **Local truth, not the catalog.** The planner targets files on disk (`whisper_file_identity`, ready
  `model_assets` rows with a file at `<managed folder>/<relative_path>`). The catalog may be empty offline.
- **Guards refuse, changing nothing.** A download of the model, a running `transcription`/`summarization`/
  `indexing` job, or a `building` index generation stops the request with a message.
- **File first, row second.** For each target: remove the file (treat `NotFound` as success, stop on any other
  error and leave that row), delete a sibling `"{file}.part"`, then delete the `model_assets` row (cascades
  `downloads`). A crash or a manual delete in between is healed because a row without a file is not installed.
- **Embedding drops the server first.** On every platform, take and `shutdown()` the embedding server before
  unlinking, because a running process keeps the file open (Windows cannot delete it). No generation server
  exists. `index_generations` and vectors are never touched, so reinstalling the same revision restores search.
- **Selection fallback only if the active variant went.**

  | Role | Another variant left | None left |
  |---|---|---|
  | whisper | activate `standard`, else the smallest | delete `transcription.whisper_selection` |
  | llm | same rule | delete `llm.generation_selection` |
  | embedding | never auto-switch | delete `llm.embedding_selection` |

  Embedding never auto-switches because another quantization has a different `artifact_revision` and would
  silently re-index; the user must choose. Removing a non-active variant changes nothing.
- **No re-download.** Uninstall never sets or clears `models.first_run_provisioned`.

Related: [0005](0005-model-catalog-and-first-run-download.md), [0006](0006-model-variant-selection.md).

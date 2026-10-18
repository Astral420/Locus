# Download whisper and embedding models on first run from a verified catalog

Locus used to bundle `whisper-small` so that capture and transcription worked on a disconnected first launch (PRD, SPEC "bundled assets", ADR 0003, `models/README.md`). Bundling costs about 466 MiB of installer size and ties every model update to an app release, so Locus now fetches models from the `locus-model-catalog` repository instead and downloads `whisper-small` and an embedding model (`all-MiniLM-L6-v2`) in the background on first launch.

**Offline first launch no longer holds for those two models.** Recording is not blocked: capture works without a model, and transcription jobs stay in the existing `blocked` state until the file arrives. A build that must work with no network can still ship the models next to the app (the resource search paths are unchanged) or set `LOCUS_NO_AUTO_DOWNLOAD`.

## Rules

- **The catalog is untrusted input.** An entry is offered for download only if it has a sha256 and size, a URL on `huggingface.co` pinned to a 40-character commit (never a branch), and a plain filename. Redirects are followed only to Hugging Face hosts over https.
- **Verify before install.** Files are hashed while they stream and are renamed into place only after size and checksum match. Interrupted downloads resume from a `.part` file.
- **A bad catalog never replaces a good one.** A fetched file replaces the cached copy only if it parses and still yields at least one verifiable model. The app reads the cache and never waits on the network.
- **First-run downloads are idempotent.** Once the defaults are present the `models.first_run_provisioned` setting stops further automatic downloads, so a user who deletes a model is not surprised by a re-download. A failure leaves the flag unset and the next launch retries.
- **Changing the embedding model changes the vectors.** Existing index generations are tied to the model id, revision and dimension, so switching models re-indexes rather than mixing vectors.
- **Selection names a variant.** Which quantization is active is decided per model in [0006](0006-model-variant-selection.md).

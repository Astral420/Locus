# Bundled model assets

> **Update:** the Whisper model is no longer required in the bundle. By default the app downloads
> `whisper-small` and an embedding model on first run from the model catalog; see
> [ADR 0005](../docs/adr/0005-model-catalog-and-first-run-download.md). Bundling it here remains
> supported for builds that must work offline from the first launch.

The standard Whisper model and the local `pyannote/speaker-diarization-community-1`
asset are release inputs, not generated during a build. M3 packaging accepts only
the checked-in or maintainer-provided local model path and performs no Hugging Face
login, download, or runtime network access. Before release, record the exact model
revision, license, checksum, and corresponding third-party notice here.

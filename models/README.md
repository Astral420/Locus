# Bundled model assets

The standard Whisper model and the local `pyannote/speaker-diarization-community-1`
asset are release inputs, not generated during a build. M3 packaging accepts only
the checked-in or maintainer-provided local model path and performs no Hugging Face
login, download, or runtime network access. Before release, record the exact model
revision, license, checksum, and corresponding third-party notice here.

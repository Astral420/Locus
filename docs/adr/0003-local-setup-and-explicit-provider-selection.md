# Separate model setup from permission to send content

Bundling every generation model would substantially increase installation size, so Locus bundles capture/transcription/diarization/slide dependencies while providing skippable generation and embedding model setup plus local import. All supported local processing works offline once its models are provisioned. Saving provider credentials makes that provider available; explicit selection authorizes the disclosed content transfer, and local failures do not automatically select a remote destination.

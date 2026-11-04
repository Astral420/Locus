# Diarize each captured source separately

**Status: proposed.** Not implemented. The pipeline still diarizes the summed `audio.wav`; this note
exists because over-splitting survives threshold tuning for recordings that capture system audio, and
per-source diarization is the architectural fix. Implementation is blocked on maintainer approval
(execution prompt B5).

## Why

`capture/audio.rs::mix_tracks_with_leads` sums every track into one mono 48 kHz `audio.wav` with no
gain staging, then hard-clips the result. `pipeline/steps.rs::audio_path` hands that single file to
the sidecar, so one clustering pass sees:

- the remote participants once, through the system-audio tap;
- the local microphone, plus whatever the speakers play back that the microphone picks up, delayed
  and quieter;
- the local room, at a different level and colour than the decoded remote stream.

Two copies of the same voice at different levels and different acoustics are exactly what a
clustering threshold cannot separate: raise the threshold and genuinely similar voices merge, lower
it and one voice splits. `docs/plans/DESIGN.md` (~line 220) already promises "Diarize both source
streams by default, retaining provenance", and capture already writes the aligned per-source files
(`aligned_file_name`), which nothing reads.

Measured on the only recordings in the repository (short `Test` captures, no ground truth): the
55 s three-source test recording yields 8 labels at the shipped settings, 5 of them under 5 s in a
recording that cannot plausibly hold 8 participants. Content of a pure-mix threshold sweep is in the
execution log; it needs a known head count to choose a value.

## Decision (proposed)

1. **Diarize the aligned per-source WAVs** (`microphone.aligned.wav`, `system_audio.aligned.wav`)
   separately when both exist, and only fall back to the mix when a single source was captured. The
   mix stays the playback/transcription file; it is no longer the diarization input.
2. **Namespace labels by source**, e.g. `microphone:SPEAKER_00` and `system:SPEAKER_00`. Two runs
   cannot share a name space without pretending the runs are comparable.
3. **Unify across sources only on voice evidence.** After echo suppression, compare per-speaker
   centroids across the two runs and merge a microphone label into a system label only above the
   same similarity threshold used for stray-label absorption (`DiarizationSettings.merge_similarity`,
   default 0.5). The result is still an anonymous label: `CONTEXT.md` is explicit that an input
   device alone does not establish a person's identity, so "microphone" never means "the user" and a
   cross-source merge never means "same person, named".
4. **Suppress echo before unification.** Microphone speech that is a delayed, attenuated copy of
   system speech is dropped from the microphone run (the system copy is cleaner: no room, no
   re-encoding). Detection works on the two aligned tracks over the candidate span: normalised
   cross-correlation peak above ~0.5 within a ±250 ms lag search, plus the microphone span's energy
   being materially below the system span's. Simultaneous speech where the microphone has its own
   independent energy and low correlation is kept — that is a real second person talking over the
   call, not an echo.
5. **Keep provenance in storage.** `DiarizationSegment.source` already carries a string and is
   written to `speaker_alignments.source`; it becomes `microphone` or `system` instead of `mixed`.
   `speakers` stays meeting-scoped, so existing display names, colours and renames keep working.
6. **No automatic re-diarization.** Meetings keep their stored alignments until the user re-runs the
   diarization step, which already replaces alignments rather than stacking them. Nothing is
   recomputed on upgrade, and no meeting is silently re-labelled.

## Consequences

- Diarization cost roughly doubles for two-source recordings, and the step's progress reporting has
  to cover two runs.
- Echo suppression is the risky part: an aggressive lag/threshold pair can drop a quiet genuine
  speaker who happens to talk while the remote side is loud. The correlation gate and the energy
  comparison are the guard, and its thresholds need real meeting audio to tune.
- The RPC contract stays backward compatible: `diarize` gains an optional per-source mode and the
  response keeps `segments`/`unavailable`, with `diagnostics` reporting each run.
- `LOCUS_DIARIZATION_*` settings apply per run; a head count is per source, not per meeting, which is
  a different thing from the per-meeting "expected speakers" hint (also unapproved).

## Alternatives considered

- **Tune the threshold only.** Shipped on this branch: `DiarizationSettings`, the sweep script and
  the optional stray-label absorption. It removes a hard-coded constant and makes the failure
  measurable, but on a summed mix with bleed it trades one failure for another.
- **Gain-stage the mix instead of splitting it.** Cheaper (no second run) but bleed still puts the
  same voice in the file twice; it cannot produce provenance.
- **Swap the embedding model** (e.g. a 3D-Speaker ERes2Net/CAM++ ONNX). A separate decision with its
  own licence and size review; it does not address duplicate voices from the mix.

## Open questions

- The cross-source merge threshold, and whether unification should also require a minimum overlap in
  speaking time.
- Whether echo suppression runs before or after stray-label absorption (before, in this sketch).
- Whether the microphone run should be skipped entirely when the system track contains no speech.

# Execution log — diarization over-splitting

Branch `fix/diarization-over-splitting`, from `dev`. Prompt: "EXECUTION PROMPT 3 of 3: Diarization
over-splitting one speaker into several".

## 0. Sync

- BASE_SHA `9134c55aa88d42521889c78f160c918115930d54` (`git switch dev`, `git pull --ff-only`).
  The analysis in the prompt was made at `5ee777d`; `dev` had moved 34 commits.
- `git diff --stat 5ee777d..HEAD` over the diarization-relevant paths: **only**
  `src-tauri/src/pipeline/steps.rs` (5 insertions, 2 deletions: the llama `--off` reasoning flag
  work). No diarization code changed, so every line reference in the prompt still held. Two
  deviations found and mapped anyway:
  - the prompt names `sidecar/src/rpc/server.py` `_diarize` — unchanged, still the handler.
  - `design/docs` reference: `docs/plans/DESIGN.md` line ~220 still promises per-source diarization.
- **Working tree was not clean at the start**: `src-tauri/locus.sqlite`, `-shm`, `-wal` are tracked
  files that are *deleted* in this checkout, before any change by me. They are a local runtime
  database, unrelated to this task; I left them untouched and never staged them. Flagged instead of
  restoring, because a deleted local database may be deliberate.
- Baseline (fresh branch, before edits):
  - `cargo test --manifest-path src-tauri/Cargo.toml` — **ok, 378 passed, 0 failed, 3 ignored**.
  - `python3 -m pytest sidecar/tests` — **19 passed, 3 skipped** (system Python has no sherpa-onnx,
    so the model-backed tests skip).
  - `.venv/bin/python -m pytest sidecar/tests` — **20 passed, 2 skipped**. The repo's `.venv` has
    sherpa-onnx 1.13.8, numpy 2.5.4 and `models/diarization/{segmentation,embedding}.onnx`, so this
    is the run that actually exercises the engine. Used from here on.
  - `pnpm build` — **ok** (`built in 2.73s`).
  - `pnpm test` — **RED at baseline**: 84 files / 149 tests fail, first error
    `TypeError: Cannot read properties of undefined (reading 'clear')` at
    `src/tests/redesign_tweaks.test.tsx:28` (`localStorage.clear()`). Pre-existing, unrelated to
    diarization (no test file touched by this task), recorded and **not** fixed per instructions.

## Phase B1 — tunable and measurable (defaults unchanged)

Files: `sidecar/src/diarization/onnx_engine.py`, `sidecar/src/diarization/engine.py`,
`sidecar/src/rpc/server.py`, `scripts/diarization-sweep.py`,
`sidecar/tests/test_diarization_settings.py`.

- `DiarizationSettings` dataclass: `threshold=0.5`, `num_speakers=-1`, `min_duration_on=0.3`,
  `min_duration_off=0.5`, `merge_gap=0.0`, `min_speaker_seconds=0.0`, `merge_similarity=0.5`.
  `from_env_and_params(params, environ=None)` reads `LOCUS_DIARIZATION_THRESHOLD`,
  `_NUM_SPEAKERS`, `_MIN_ON`, `_MIN_OFF`; RPC params win over the environment; clamps are
  threshold 0.1–1.5, `num_speakers` −1 or 1–20, durations 0–5 s, similarity 0–1. Unparseable values
  fall back to the default instead of failing the step (a bad tuning value must not cost labels).
- `diarize_onnx_with_diagnostics(audio_path, progress, settings)` returns
  `{"segments": [...], "diagnostics": {"engine", "speakers": [{speaker, seconds, segments}],
  "settings", "audio_seconds"}}`; `diarize_onnx` keeps its old list return type by calling it.
  `num_speakers > 0` maps to `FastClusteringConfig.num_clusters` (the threshold only applies when
  clustering is automatic).
- `engine.diarize_with_diagnostics(...)` is the new entry point; `engine.diarize(...)` keeps its
  signature plus an optional `settings` kwarg and returns the segment list. The pyannote fallback
  reports its own `diagnostics` with empty `settings` (its defaults are internal).
- `rpc/server.py::_diarize` forwards `params["settings"]` when it is a dict and returns
  `{"segments", "diagnostics"}`; the `unavailable` path is unchanged, so
  `test_rpc_reports_diarization_unavailable_instead_of_fake_speakers` still passes.
- Backward compatibility both ways: the Python response only *adds* `diagnostics`; Rust
  `DiarizationResponse` has no `deny_unknown_fields`, and a new `#[test]` pins that (see B4).
- `scripts/diarization-sweep.py`: 45-run grid (threshold 0.5/0.6/0.7/0.8/0.9 × min_on 0.3/0.6/1.0 ×
  min_off 0.5/0.8/1.2), prints speaker count, seconds per speaker, labels under 5 s, turn changes,
  marks `--expected-speakers` matches, ends with one recommendation line and how to apply it.
  Optional `--min-speaker-seconds` / `--merge-gap` measure post-processing in the same run.
- New tests `test_diarization_settings.py` (9 cases): defaults, env parsing, param-over-env
  precedence, clamping, automatic speaker count, junk values, non-mapping payload, extremes.

## Phase B2 — post-processing

Files: `sidecar/src/diarization/postprocess.py` (new), wiring in `onnx_engine.py`,
`sidecar/tests/test_diarization_postprocess.py` (new, 10 cases).

- `merge_adjacent(segments, max_gap)` joins consecutive same-speaker, same-source segments within
  the gap; `speaker_totals`, `speaker_summary`, `turn_changes` back the diagnostics and the sweep.
- `absorb_tiny_speakers(segments, min_seconds, min_fraction, centroids=None, merge_similarity=0.5)`:
  floor is `max(min_seconds, min_fraction * total speech)`; strays are processed smallest first and
  only folded into speakers above the floor. Decision order is centroid cosine similarity when both
  labels have embeddings, otherwise temporal adjacency. It refuses two cases: the stray overlaps its
  only alternative for more than half its duration (that is simultaneous speech, not a copy), and a
  stray with a comparable centroid that matches nobody above `merge_similarity` (a real quiet
  participant). Labels are re-ordered with `relabel_by_first_appearance`, which moved to
  `postprocess.py` and is re-exported from `onnx_engine.py` so the existing import in
  `test_engines.py` keeps working.
- Wiring in `diarize_onnx_with_diagnostics`: only when `min_speaker_seconds > 0`; one averaged
  embedding per label from `sherpa_onnx.SpeakerEmbeddingExtractor` (same model file, 16 kHz slices of
  the prepared audio, slices under 0.5 s skipped), then `merge_adjacent`. A broken extractor only
  loses the similarity tie-break (logged, falls back to adjacency).

## Phase B3 — cleaner input (sidecar only)

Files: `sidecar/src/diarization/onnx_engine.py`, `sidecar/tests/test_read_mono_16k.py` (new).

- `read_mono_16k` now does band-limited rational resampling (numpy only) instead of averaging groups
  of 3 samples: Kaiser-windowed sinc, passband to 7 kHz, stopband from 8 kHz, 50 dB down, exact unit
  gain per polyphase branch, silence padding so the filter window is always in-array. The 16 kHz
  fast path is unchanged, and the signature/dtype are unchanged.
  - Measured while designing: 9 kHz at 44.1/48 kHz input lands at about −100 dB (the old averaging
    let it fold to 7 kHz at roughly −0.5 dB); 1 kHz level is within 0.01 dB.
  - Cost: about 140 taps per output sample for 48 kHz, so a 30-minute 48 kHz file resamples in
    ~55 s (measured on this machine with a 175-tap variant). Fine relative to diarization itself,
    but worth knowing.
- `normalise_for_diarization` scales a *copy* to −23 dBFS RMS with a 0.95 peak ceiling, applied in
  `diarize_onnx_with_diagnostics` before the engine and before the embedding extractor. Nothing is
  written back to disk. Silence is left silent rather than amplified.
- New tests: 48 kHz 1 kHz level within 1 dB, 9 kHz attenuated >40 dB in the steady band, 44.1 kHz
  length and level, 8 kHz upsampling, 16 kHz passthrough, stereo downmix, non-16-bit refusal,
  normalisation target/ceiling/no-mutation/silence.

## Phase B4 — alignment on per-speaker total overlap (Rust)

File: `src-tauri/src/pipeline/diarization.rs`.

- `align_transcript_segment` keeps its signature and now sums overlap per speaker label, picks the
  largest total, returns that total as the coverage numerator and `uncertain = total/duration < 0.5`.
  Ties go to the earlier start, so re-runs are stable. `partial_overlap_is_marked_uncertain` passes
  unchanged.
- New `aligned_span(start, end, speaker, candidates)` returns the union of that speaker's
  overlapping intervals clipped to the segment; `persist_alignments` stores that span instead of the
  single interval's slice.
- New tests (7): two short turns beat one long turn, the sum only wins when it is actually larger,
  tie → earliest speaker regardless of input order, no overlap → `None`, summed overlap decides
  uncertainty (2 s of 4 s is exactly half), stored span is the union (and clips), the span matches the
  alignment across a mid-sentence change. Plus 2 producer/consumer tests: a response carrying
  `diagnostics` still parses, and a segment without `source` still defaults to `mixed`.

## Phase B6 — choosing defaults: **not changed**

There is audio in the repository (`src-tauri/media/*/`, 11 recordings, 2–55 s, all three sources) but
**no ground truth**: every meeting is a deleted "Test"/"Untitled Conversation" capture with no stored
diarization, so the head count is unknown. Choosing a global threshold from it would be guessing.

Sweep, `src-tauri/media/7b90c9e3-.../audio.wav` (54.7 s, mic + system + screen), default
post-processing off, 45 runs (abridged: default row first, then the plateau edges):

| threshold | min_on | min_off | speakers | seconds per speaker | under 5 s | turns |
|---|---|---|---|---|---|---|
| 0.5 | 0.3 | 0.5 | 8 | 12/7/6/4/2/1/1/0 | 5 | 12 |
| 0.5 | 1.0 | 0.5 | 5 | 12/7/6/4/1 | 2 | 4 |
| 0.6 | 0.6 | 0.5 | 5 | 14/7/6/4/1 | 2 | 4 |
| 0.7 | 1.0 | 0.5 | 4 | 14/12/4/1 | 2 | 3 |
| 0.8 | 1.0 | 0.5 | 2 | 17/15 | 0 | 1 |
| 0.9 | 1.0 | 1.0 | 2 | 17/15 | 0 | 1 |

With `--min-speaker-seconds 4.0 --merge-gap 0.8` the default row only moves 8 → 7 labels, so the
post-processing alone does not repair a mix-level over-split; the threshold is the lever, and it is
the lever that needs a known head count.

Decision: ship the tooling with **defaults unchanged** (`merge_gap=0.0`, `min_speaker_seconds=0.0`,
`threshold=0.5`). What the maintainer must run, on a real meeting with a known head count:

```
.venv/bin/python scripts/diarization-sweep.py path/to/meeting/audio.wav --expected-speakers 2
```

then apply the recommended triple through `LOCUS_DIARIZATION_THRESHOLD` / `_MIN_ON` / `_MIN_OFF`
(or the `settings` RPC parameter) and re-run before making it a default.

## Phase B5 — per-source diarization: design note written, **implementation waiting for approval**

`docs/adr/0009-diarize-per-source.md` (proposed): separate runs over the aligned per-source WAVs,
source-namespaced labels unified only on voice evidence, echo suppression by cross-correlation and
energy before unification, `source` provenance in `speaker_alignments`, and no automatic
re-diarization of existing meetings. Nothing in that ADR is implemented, and no per-meeting
"expected speakers" UI hint was added.

## Suites after the work

- `.venv/bin/python -m pytest sidecar/tests` — **52 passed, 2 skipped** (was 20/2; +32 new).
- `python3 -m pytest sidecar/tests` — see final report; the system interpreter lacks sherpa-onnx, so
  model tests skip there.
- `cargo test --manifest-path src-tauri/Cargo.toml` — **385 passed, 0 failed, 3 ignored** (was 378/0).
- `cargo test ... pipeline::` — green, including the 9 new diarization tests.
- `pnpm build` — ok. `pnpm test` — still red at baseline (149 failures, unrelated).

## Found but not fixed

1. **`pnpm test` is red on `dev`**: 84 files / 149 tests, `localStorage` undefined in
   `redesign_tweaks.test.tsx` (and 83 more files). Out of scope; no frontend file was touched.
2. **The three tracked `src-tauri/locus.sqlite*` files are deleted in this checkout** (pre-existing).
   Whether that is deliberate cleanup or an accident is the maintainer's call.
3. **`mix_tracks_with_leads` sums with no gain staging and hard-clips** at 48 kHz — the prompt's
   finding #3. Left alone: `capture/*` is explicitly out of scope (B.8), and the fix belongs with the
   per-source work.
4. **`docs/plans/DESIGN.md` disagrees with the code**: it says pyannote-audio with
   `speaker-diarization-community-1` (lines ~128, ~471–485), while the code uses sherpa-onnx +
   pyannote segmentation 3.0 + NeMo TitaNet-small. `PRODUCT.md` also says "speaker diarization
   (pyannote)". Documentation, not behaviour; not edited here.
5. **`speaker_alignments` writes one row per transcript segment** even when a speaker's intervals are
   discontiguous inside it: the stored span is now the union, so the span can cover a gap where the
   other person talked. That is the prompt's requested behaviour, noted because a UI drawing the span
   as "this person spoke here" will slightly overstate. A per-interval row would need a schema
   decision.
6. **Diarization cost of resampling**: a 30-minute 48 kHz file spends ~55 s in the anti-alias filter
   before the engine starts. Acceptable today; chunking or a shorter filter would cut it.
7. **No expected-speaker plumbing**: `num_speakers` is settable per call and via env, but nothing in
   the app exposes it (deliberately, per B.7).

## Final report

1. Base `9134c55aa88d42521889c78f160c918115930d54`; final `3eec82e8da46c3c1df159ac3b936ab6cfa0f280a`.
   `dev` did not move while I worked (`origin/dev` was still `9134c55` at the end), so no rebase.
   Branch `fix/diarization-over-splitting`.
2. Commits: `8f8a438` sidecar (B1–B3), `eaa440a` Rust alignment (B4), `3eec82e` ADR + this log
   (B5/B6 record). Files per phase are listed above. Tests added: 9 settings, 10 post-processing,
   10 input/resampling, 9 Rust (7 alignment + 2 response shape).
   Suites: pytest `.venv` 20 → 52 passed (2 skipped both times); pytest system `python3` 19 → 51
   passed (3 skipped both times); cargo 378 → 387 passed, 0 failed, 3 ignored; `pnpm build` ok → ok;
   `pnpm test` red at baseline (149 failures) and unchanged — no frontend file was touched.
3. Sweep table above. Defaults unchanged (threshold 0.5, `merge_gap` 0, `min_speaker_seconds` 0)
   because the only audio in the repository has no known head count.
4. Docs that disagree with the code: `docs/plans/DESIGN.md` (pyannote-audio +
   `speaker-diarization-community-1`, lines ~128 and ~471–485) and `PRODUCT.md` ("speaker
   diarization (pyannote)") versus sherpa-onnx + pyannote segmentation 3.0 + TitaNet-small.
   Not edited: documentation, separate change.
5. Per-source diarization: `docs/adr/0009-diarize-per-source.md` written as a proposal; nothing
   implemented, approval pending. No "expected speakers" UI hint added.
6. Nothing from the prompt's findings had been fixed on the current `dev`; only `steps.rs` had moved
   since `5ee777d`, and every referenced function and file still existed under its old name.
7. "Found but not fixed" above (items 1–7).

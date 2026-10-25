# Execution log — capture test FFmpeg capability fix

Prompt: `EXEC-1-capture-tests.md` (fix five failing `capture::*` tests without editing tests).

## Phase 0 — Sync and baseline

- `git fetch origin --prune`, `git switch dev`, `git pull --ff-only origin dev`.
- BASE_SHA: `b522d6303557314cdd79b3def92a70de33b199fc` (dev was already at this HEAD; `5ee777d..HEAD` = b522d63, 08dcfea, 5f85b71, 0177cd4, 76b57e0, 1635ed4).
- `git diff --stat 5ee777d..HEAD -- src-tauri/src/capture .github/workflows packaging src-tauri/ffmpeg APPLY.md`: only `release.yml` and `packaging/llama-engine/*` changed. Capture code, FFmpeg workflow, `src-tauri/ffmpeg`, and `APPLY.md` are unchanged since the prompt's analysis commit, so the line references below were re-verified against HEAD and still match.
- Working tree clean. Branch `fix/capture-test-ffmpeg` created from `dev`.

### Diagnosis confirmation

```
$ ls -l src-tauri/ffmpeg/ffmpeg
-rwxr-xr-x ... 7374120 ... src-tauri/ffmpeg/ffmpeg

$ src-tauri/ffmpeg/ffmpeg -muxers | grep -E ' (s16le|rawvideo) '
EMPTY

$ src-tauri/ffmpeg/ffmpeg -encoders | grep -E ' (rawvideo|pcm_s16le|libx264|aac) '
libx264, aac, pcm_s16le   (no rawvideo)

$ src-tauri/ffmpeg/ffmpeg -devices
(no devices; no lavfi)
```

`cargo test capture::` -> 96 passed, 5 failed (exactly the five named tests).
`LOCUS_FFMPEG=$(which ffmpeg) cargo test capture::` -> 101 passed, 0 failed (system FFmpeg 9.0.2).
Diagnosis confirmed.

### Baseline suites (before edits)

| Command | Result |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | 315 passed, **5 failed** (the five `capture::*`), 2 ignored |
| `pnpm test` | **pre-existing red**: 84/93 files failed, 149/257 tests failed (`localStorage` undefined in test env). Unrelated; not fixed. |
| `pnpm build` | pass |
| `python3 -m pytest sidecar/tests` | 19 passed, 3 skipped |

Other baseline failures are pre-existing and out of scope.

## Phase 1 — Fix (`src-tauri/src/capture/screen.rs`)

Implemented in `screen.rs` (non-test production path unchanged):

- `locate_ffmpeg()` now splits into `#[cfg(test)]` → `locate_test_ffmpeg()` and
  `#[cfg(not(test))]` → the original candidate search, byte-for-byte.
- Test selection order: `LOCUS_TEST_FFMPEG`, `LOCUS_FFMPEG`, bundled/app
  candidates, cwd `ffmpeg/ffmpeg`, `src-tauri/ffmpeg/ffmpeg`, system paths
  (`/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`), then `PATH`. Duplicate
  candidates are removed before probing. Selection is cached in
  `OnceLock<Option<PathBuf>>`.
- Added pure `missing_test_capabilities(muxers, encoders, devices, filters)` and
  `listing_has()`: they parse the flags/name columns only, so words in headings
  or descriptions (e.g. `rawvideo passthrough`) do not count. Required:
  muxers `s16le`,`rawvideo`; encoders `rawvideo`,`pcm_s16le`,`libx264`,`aac`;
  device `lavfi`; filter `testsrc`.
- `supports_unit_tests()` runs `-muxers/-encoders/-devices/-filters` with stdin
  null and captured output; a binary that cannot run/does not exit 0 reports
  every capability missing.
- Skipped candidates print one line each; when nothing qualifies a final line
  says FFmpeg-backed tests will be skipped.
- New module `ffmpeg_capability_tests` covers minimal/full listings, the
  description false-positive, un-runnable binaries and `#[cfg(unix)]` fake
  FFmpeg shell scripts (Err for minimal/failing, Ok for full).

No existing `#[cfg(test)]` module or test helper was edited. `git diff -U0`
hunks touch only lines 22-270 and append after old line 1084 (the end of the
pre-existing module).

### Commands and results

| Command | Result |
|---|---|
| `cargo test ... ffmpeg_capability_tests` | 6 passed |
| `cargo test ... capture::` (bundled minimal) | **107 passed, 0 failed**; prints `locus test: skipping ffmpeg/ffmpeg: it lacks s16le, rawvideo, lavfi, testsrc...` then selects `/usr/local/bin/ffmpeg` |
| `LOCUS_TEST_FFMPEG=$(which ffmpeg) cargo test ... capture::` | **107 passed, 0 failed**, no skip message |
| `cargo test --manifest-path src-tauri/Cargo.toml` | **326 passed, 0 failed, 2 ignored** |
| `rustfmt --check --edition 2021 src-tauri/src/capture/screen.rs` | clean |

Note: the skip message shows the candidate as spelled in the list
(`ffmpeg/ffmpeg`, since cargo test's cwd is `src-tauri/`); it maps to
`src-tauri/ffmpeg/ffmpeg`.

## Phase 2 — Docs

- `src-tauri/ffmpeg/README.md`: new "Running the unit tests" section (minimal
  build is deliberate, automatic skip + message, `LOCUS_TEST_FFMPEG` override,
  `ffprobe` on `PATH`).
- `APPLY.md`: replaced `export LOCUS_FFMPEG=$(which ffmpeg)` with the
  `LOCUS_TEST_FFMPEG` unit-test guidance and kept `LOCUS_FFMPEG` for running the
  app against a system FFmpeg.

`pnpm build` passed after the changes. `pnpm test` remains red with the same
pre-existing 84-file `localStorage` failure set recorded at baseline.

## Phase 3 — Final verification and report

- Re-checked upstream: `git log b522d63..origin/dev` is empty; `origin/dev` is
  still `b522d63`. No rebase was needed.
- Final implementation SHA: `abdaee04c3ea5421a6d9b719d3d6326989ab61f4`
  (this log file is committed after it).
- Final branch: `fix/capture-test-ffmpeg`.

### Final suite results

| Command | Result |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | **326 passed, 0 failed, 2 ignored** |
| `cargo test ... capture::` (bundled minimal selected/skipped) | **107 passed, 0 failed** |
| `LOCUS_TEST_FFMPEG=$(which ffmpeg) cargo test ... capture::` | **107 passed, 0 failed**, no skip message |
| `pnpm build` | pass |
| `pnpm test` | **pre-existing red**: 84/93 files, 149/257 tests (same as baseline; `localStorage` undefined) |
| `python3 -m pytest sidecar/tests` | 19 passed, 3 skipped |

### Diagnosis output and selection

```
src-tauri/ffmpeg/ffmpeg -muxers | grep -E ' (s16le|rawvideo) '   -> empty
src-tauri/ffmpeg/ffmpeg -encoders                                 -> libx264, aac, pcm_s16le (no rawvideo)
src-tauri/ffmpeg/ffmpeg -devices                                  -> no devices (no lavfi)
cargo test capture::                                              -> 96 passed, 5 failed
LOCUS_FFMPEG=$(which ffmpeg) cargo test capture::                 -> 101 passed, 0 failed
```

During `cargo test ... capture::` the selected test FFmpeg was
`/usr/local/bin/ffmpeg` (full build, version 9.0.2). Skipped candidate:
`ffmpeg/ffmpeg` (cwd `src-tauri/`, i.e. the bundled minimal
`src-tauri/ffmpeg/ffmpeg`), lacking `s16le, rawvideo, lavfi, testsrc`.
The minimal build actually also lacks the `rawvideo` encoder, but `rawvideo`
is deduplicated in the capability list.

### Existing tests untouched

`git diff -U0 -- '*.rs'` against `b522d63` shows hunks only at old lines
22-270 and an insertion after old line 1084 (end of `screen.rs`), plus the new
`ffmpeg_capability_tests` module. No line inside any pre-existing
`#[cfg(test)]` module changed. `container.rs` and `video_pipeline.rs` are
untouched.

### Other tests needing a full FFmpeg

None found. Other FFmpeg-backed tests (`recovery`, `native_screen`,
`video_encoder`, `encoder`) use only demuxers/encoders present in the minimal
build and passed at baseline. `video_encoder.rs` uses `-f rawvideo` as an
input demuxer (present), not as a muxer.

### Found but not fixed

- `pnpm test` is broadly red at baseline in this environment
  (`localStorage`/`window.localStorage` undefined, 84 files / 149 tests).
  Pre-existing and unrelated; not touched.
- The prompt's expected skip message says `src-tauri/ffmpeg/ffmpeg`, but cargo
  test runs with `src-tauri/` as cwd, so the candidate is printed as
  `ffmpeg/ffmpeg`. Same file; left as the natural relative spelling.

### Uncertainties

- `ffprobe` is not bundled; `container.rs` tests call `Command::new("ffprobe")`
  directly, so they rely on a system `ffprobe` on `PATH`. That was already true
  and is documented in the new README section.

# Execution log: detect screen captures and keep their OCR out of the summary (5c)

## Sync

- Base SHA: `198dfcacc9b42fc2f436016acaf61d9adb6cc9a1` (`dev`).
- Branch: `fix/summary-slide-source-detect`.
- `git status --short` before work: clean.
- `dev` had moved from the analysis SHA `9134c55` to `198dfca`. The move included 5a (`fb695a7`, prompt stats) and 5b (`6731a5c`, OCR cleaning), so the analysis was re-read against the current code.
- Adaptations required by the move:
  - `generate_payload` already cleans slides through `slide_text::clean_slides`; the classifier runs on the **raw** `SlideRow` OCR, as 5b requires, and the existing cleaned text is only used when the decision is `Presentation`.
  - The 5a `PromptStats` struct was extended with `slide_source` and `slide_reason` instead of adding a parallel diagnostics path.
  - The loader already ordered slides chronologically; no ordering change was needed here.
- `git fetch origin` at the end showed `origin/dev` still at `198dfca`; no rebase was needed.
- No name from the brief was missing; all referenced symbols (`SlideRow`, `SummaryContext`, `generate_payload`, `validate_payload`, `GeneratedSummary.disclosures`, `slide_occurrences`) exist.

## Baseline (before changes)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 401 passed, 0 failed, 3 ignored |
| `pnpm build` | PASS: `tsc --noEmit` and `vite build` completed |
| `pnpm test` | FAIL (pre-existing/environmental): 84 files / 149 tests failed because Vitest collects vendored `packaging/llama-engine/.work/.../llama.cpp/tools/ui` tests and `localStorage` is undefined |

## Phase 1: pure slide-source classifier

Files changed:

- `src-tauri/src/llm/slide_source.rs` (new)
- `src-tauri/src/llm/mod.rs` (register module)

Commit: `b97d9dd63b246e797cc2a6ac7edbbebdcdbc88ab`.

Decisions:

- `SlideSourceStats { unique_slides, total_chars, max_chars, total_lines, chrome_like_lines, occurrences, duration_seconds }` and `SlideSourceDecision { kind, reason }` with `SlideSourceKind { Presentation, ScreenCapture }`.
- `build_stats(slides, occurrences, duration_seconds)` counts raw OCR characters/lines and clamps a negative duration to zero.
- `classify_slide_source` rules (constants at the top of the module):
  - `avg_chars = total_chars / unique_slides`.
  - `ScreenCapture` if `avg_chars >= 1_200`; or `avg_chars >= 700` and `chrome_like_lines / total_lines >= 25%`; or `changes_per_minute >= 8` and `avg_chars >= 400` (rate only when duration is known).
  - Otherwise `Presentation`; zero slides returns `Presentation` with reason `"no slides"`.
- The safe-failure direction is documented in the module header: excluding real slides loses detail but keeps the speech; including screen chrome corrupted the summary.
- `chrome_like_line` is language-agnostic: `://`/`www.` substrings, `/`, `~/`, drive paths (`C:\`, `C:/`), `$ `, `% `, `> ` prefixes, and file names ending in `.rs .ts .tsx .js .py .json .html .css .md .toml`. The signal arrays are constants (`CHROME_SUBSTRINGS`, `CHROME_PREFIXES`, `CHROME_EXTENSIONS`) so they can be tuned in one place.
- The chrome percent uses `saturating_mul(100).checked_div(total_lines).unwrap_or(0)` so there is no division by zero and no clippy `manual_checked_division` warning.

Tests added (12, in `llm::slide_source::tests`):

- `the_measured_screen_recording_is_a_screen_capture`
- `a_typical_deck_is_a_presentation`
- `a_text_heavy_lecture_deck_is_a_presentation`
- `a_browser_session_is_a_screen_capture`
- `rapid_changes_with_moderate_text_are_a_screen_capture_only_when_fast`
- `no_slides_is_a_presentation_without_panicking`
- `density_and_chrome_thresholds_are_inclusive_at_the_limit`
- `the_rate_threshold_is_inclusive_at_the_limit`
- `chrome_like_lines_cover_urls_paths_prompts_and_code_files`
- `stats_are_built_from_raw_slide_rows`
- `a_negative_duration_is_treated_as_unknown`
- `empty_rows_do_not_panic_or_divide_by_zero`

Existing tests or assertions changed: none.

## Phase 2: use the decision in `generate_payload`

Files changed:

- `src-tauri/src/llm/summarization.rs`

Commit: `994977e2d406f0f1e4abf7de93decec48606a337`.

Decisions:

- `SummaryContext` now carries `slide_occurrences`, loaded with `SELECT COUNT(*) FROM slide_occurrences WHERE meeting_id = ?1`. It is not stored anywhere new and no migration was added.
- `meeting_duration_seconds` uses the last transcript segment end, falling back to the last slide timestamp; `0` disables the rate rule.
- In `generate_payload`, before composing `slide_text`, the raw rows are classified. On `ScreenCapture` the prompt gets an empty slide text (so the existing prompt already renders `(none)`), and one stderr line is printed: `locus: slide text not used (screen capture): <reason>`.
- On `Presentation` the existing cleaned slide text is used unchanged, so 5b's cleaning still applies.
- `PromptStats` gained `slide_source` and, when known, `slide_reason="<reason>"`, appended to both the stderr and diagnostic lines.
- `validate_payload` was **not** changed: citations are still checked against every real slide row, so existing stored summaries that cite slides keep validating even though the model cannot cite slides when the text is excluded.
- Optional disclosure: `SHOW_SLIDE_FILTER_DISCLOSURE: bool = false` is the only switch. When true, a `ScreenCapture` payload gets `"Screen text was not used because the recording looks like a screen capture rather than slides."` appended to `disclosures`; the existing markdown already renders it. Default `false` means nothing user-visible changes. `slide_filter_disclosure(enabled, kind)` takes the flag as an argument so a test can exercise both settings.
- No frontend, DTO, contract, migration, OCR storage, sidecar, knowledge-base or capture code was touched.

> Existing test fixture changed on purpose (assertions unchanged): `oversized_slide_text_exhausts_the_fold_loop` used one ~14.5k-character browser/terminal slide. That is now correctly classified as a screen capture, so the fixture would no longer reach the fold loop. The fixture was replaced with a 12-slide text-heavy deck of unique prose lines (average ~990 characters per slide) that stays `Presentation` and still exceeds `MAX_SINGLE_CONTEXT_CHARS`. The four assertions are unchanged. This is reported here as the one existing-test change the brief allows.

Tests added (4, in `llm::summarization::tests`):

- `a_screen_capture_prompt_keeps_speech_and_drops_slide_text`
- `a_presentation_prompt_still_carries_slide_text`
- `the_slide_filter_disclosure_is_opt_in`
- `the_meeting_duration_prefers_the_transcript_and_falls_back_to_slides`

## Verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` (after Phase 1 formatting) | PASS: 417 passed, 0 failed, 3 ignored (baseline 401; 16 new tests) |
| `cargo test --manifest-path src-tauri/Cargo.toml` (final) | PASS: 417 passed, 0 failed, 3 ignored |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` | PASS (warnings only, all pre-existing; no warning in `slide_source.rs` after the `checked_div` fix) |
| `rustfmt --edition 2021 --check src-tauri/src/llm/slide_source.rs src-tauri/src/llm/summarization.rs` | PASS |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing `localStorage`/vendored-test environment issue as baseline: 84 files / 149 tests) |
| `git diff --check` | PASS |
| `git diff --stat -- src` | EMPTY |

## Thresholds

| Constant | Value | Rule |
| --- | --- | --- |
| `SCREEN_AVG_CHARS` | 1,200 | screen capture at or above this average density |
| `SCREEN_AVG_CHARS_WITH_CHROME` | 700 | density needed for the chrome rule |
| `SCREEN_CHROME_PERCENT` | 25 | chrome-like lines, in percent, that turn the rule on |
| `SCREEN_CHANGES_PER_MINUTE` | 8.0 | frame changes per minute needed for the rate rule |
| `SCREEN_RAPID_AVG_CHARS` | 400 | minimum density for the rate rule |

- The user's measured case (5 slides, 12,181 characters total, 3,329 largest) classifies as `ScreenCapture` by the first rule (2,436 average characters per slide).
- No real recording was available to run the backend against locally; the checks are the documented counts from the bug report plus pure fixtures. Manual Gemma/Qwen regeneration on the 5-slide meeting is left to the user as specified.

## Found but not fixed

- `pnpm test` remains red on this machine for the pre-existing environment reason recorded in the baseline and in 5a/5b.
- The fold loop can still exhaust for genuinely huge presentation decks; the budget is 5d.
- The prompt still places slide text after the transcript with the same untrusted label and never mentions slides in the system prompt; that is 5e.
- The slide detector still saves a frame on every large change; that is outside this task and was not touched.
- The threshold values were tuned against the reported meeting and synthetic fixtures, not against a corpus of real recordings.

## Final report

### SHAs and sync

- Base SHA: `198dfcacc9b42fc2f436016acaf61d9adb6cc9a1`.
- Phase 1 commit: `b97d9dd63b246e797cc2a6ac7edbbebdcdbc88ab`.
- Phase 2 commit: `994977e2d406f0f1e4abf7de93decec48606a337`.
- Final branch tip: the docs commit that records this report.
- `dev` moved from `9134c55` to `198dfca` before work started (5a and 5b merged). The implementation was adapted to the merged code, not redone. `origin/dev` did not move afterwards and no rebase was required.

### Files changed per phase

Phase 1:

- `src-tauri/src/llm/slide_source.rs`: new pure classifier, stats builder, chrome-line detection and 12 unit tests.
- `src-tauri/src/llm/mod.rs`: module registration.

Phase 2:

- `src-tauri/src/llm/summarization.rs`: occurrence count in the context, duration helper, classification and filtering in `generate_payload`, stats fields, optional disclosure helper, 4 tests, and the adapted fold-loop fixture.

Phase 3:

- `docs/dev/EXECUTION_LOG_summary_slide_source.md`: this ledger.

### Tests added

16 tests listed in the phase sections above.

### Existing assertion changed

None changed. One existing test *fixture* was changed on purpose (`oversized_slide_text_exhausts_the_fold_loop`) because it was shaped like a screen capture and is now filtered before the fold loop; the assertions are identical.

### Uncertainty

- The chrome-like detector is intentionally broad (a leading `/`, common code extensions). A real slide that happens to contain a URL or file name is still only `ScreenCapture` if it also reaches the density/chrome thresholds, but the exact boundary was not measured on real decks.
- The rate rule depends on the transcript duration; a meeting whose transcript ends long before the screen activity will under-count changes per minute. The fallback to the last slide timestamp only applies when there is no transcript end.
- `PromptStats` always records the decision and reason. The reason is a short generated sentence, not text from the meeting, so it cannot leak transcript or OCR content.

# Execution log: clean OCR slide text before prompting (5b)

## Sync

- Base SHA: `22a8501037d87c78df2a74faf28f0d19261466a6` (`dev`).
- Branch: `fix/summary-ocr-clean`.
- `git status --short` before work: clean.
- `dev` had moved from the analysis SHA `9134c55` to `22a8501`. The move included `fb695a7` (5a, prompt stats), so the analysis was re-read against the current code and the stats shape was extended instead of being rebuilt.
- `git diff --stat 9134c55..HEAD -- src-tauri/src/llm/summarization.rs src-tauri/src/llm/mod.rs` was non-empty only because of 5a; the slide query, `SlideRow`, `generate_payload` and `validate_payload` still matched the analysis.
- No rebase was needed afterwards; `origin/dev` was still `22a8501` at the end.

## Baseline (before changes)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 392 passed, 0 failed, 3 ignored |
| `pnpm test` | FAIL (pre-existing/environmental): Vitest collects vendored `packaging/llama-engine/.work/.../llama.cpp/tools/ui` tests and `localStorage` is undefined. 84 files / 149 tests failed. |
| `pnpm build` | PASS: `tsc --noEmit` and `vite build` completed. |

## Phase 1: pure slide-text cleaning and chronological ordering

Files changed:

- `src-tauri/src/llm/slide_text.rs` (new)
- `src-tauri/src/llm/mod.rs` (register module)
- `src-tauri/src/llm/summarization.rs` (loader query, `SlideRow` visibility, `generate_payload`, stats, tests)
- `docs/dev/EXECUTION_LOG_summary_ocr_clean.md` (this ledger)

Decisions:

- Added `slide_text::clean_slides(&[SlideRow]) -> CleanedSlides` with `CleanedSlide { id, timestamp_seconds, lines }`, `CleanStats { slides_in, slides_out, lines_in, lines_out }`.
- Rules are applied deterministically and char-based: trim/collapse whitespace; require at least two `is_alphabetic` characters; comparison key lowercases, maps `is_numeric` characters to `#`, and collapses whitespace.
- Chrome is a key on at least 3 slides, or on at least 40% of slides when there are 3+ slides (integer arithmetic: `count * 5 >= slides_in * 2`).
- Cross-slide duplicates keep the first occurrence globally.
- Near-duplicate merging compares the signatures captured *after chrome removal but before cross-slide dedupe*. Comparing only surviving lines would always look dissimilar because the shared lines were already dropped. Merging appends the later slide's unique lines to the earlier slide, keeps the earlier id/timestamp, and updates the comparison signature to the latest merged slide so consecutive runs merge correctly.
- Slides left with zero lines are dropped.
- `SlideRow` is now `pub(crate)` (fields too) so the new module can consume it without moving the type.
- The slides query now orders by the first occurrence timestamp, then its ordinal, then id: `ORDER BY 2, (SELECT so.ordinal ... LIMIT 1), s.id`. Columns are unchanged.
- `generate_payload` formats `CleanedSlides` as `[id @ timestamp] line1\nline2...`, the same bracket shape as before, so citations keep working.
- `validate_payload` was not changed: it still checks citations against every real row in `context.slides`, including slides merged or dropped from the prompt.
- 5a stats were extended with `slide_lines_out/slide_lines_in` and rendered as `slide_lines=<out>/<in>`. `slide_chars_available` is now the raw formatted OCR length, while `slide_chars` remains the cleaned text actually sent, so the next fix can report the difference.
- Stored OCR rows, knowledge-base indexing, the OCR sidecar, prompts, the budget loop and the frontend were not touched.

Tests added:

- `llm::slide_text::tests::repeated_chrome_disappears_from_every_slide`
- `llm::slide_text::tests::a_cross_slide_duplicate_keeps_only_the_first_occurrence`
- `llm::slide_text::tests::digit_variants_compare_equal_and_clock_like_lines_vanish`
- `llm::slide_text::tests::near_duplicate_consecutive_slides_merge_but_dissimilar_ones_do_not`
- `llm::slide_text::tests::multilingual_lines_survive_intact`
- `llm::slide_text::tests::cleaning_clean_data_is_idempotent_and_empty_input_stays_empty`
- `llm::slide_text::tests::browser_heavy_slides_shrink_substantially`
- `llm::summarization::tests::slides_reach_the_prompt_in_chronological_order`
- `llm::summarization::tests::citations_of_slides_dropped_from_the_prompt_stay_valid`

Existing tests or assertions changed: none.

## Phase 1 verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 401 passed, 0 failed, 3 ignored (baseline 392; 9 new tests) |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing `localStorage`/vendored-test environment issue as baseline) |
| `git diff --check` | PASS |
| `rustfmt --edition 2021 --check` on the three changed Rust files | PASS |

Sample fixture stats (the `browser_heavy_slides_shrink_substantially` fixture: 5 slides, ~2,400 characters each, 6 shared chrome lines per slide plus 40 unique browser-like lines): `lines_in = 230`, `lines_out = 200`; all 30 chrome lines removed. The fixture asserts `lines_out < lines_in`.

## Found but not fixed

- `pnpm test` remains red on this machine for the pre-existing environment reason recorded in 5a. The frontend was not touched.
- The fold loop can still exhaust with oversized cleaned OCR; the budget is 5d.
- The prompt still places slide text after the transcript with the same untrusted label and never mentions slides in the system prompt; that is 5e.
- The slide detector still treats every >12% frame change as a slide; that is 5c.
- Manual acceptance (the user's 5-slide / 12,181-character meeting) was not run here; no local model run was performed.

## Final report

### SHAs and sync

- Base SHA: `22a8501037d87c78df2a74faf28f0d19261466a6`.
- Implementation commit: `6731a5cf88f5f79c50651733789d46e0fff4f53b`.
- Final branch tip: the docs commit that records this report.
- `dev` moved from `9134c55` to `22a8501` before work started (5a was merged). The code was adapted, not redone. `origin/dev` did not move afterwards and no rebase was required.

### Files changed per phase

Phase 1:

- `src-tauri/src/llm/slide_text.rs`: new pure cleaning module and its unit tests.
- `src-tauri/src/llm/mod.rs`: module registration.
- `src-tauri/src/llm/summarization.rs`: chronological slide query, `pub(crate)` slide row, cleaned prompt assembly, line-count stats, loader/citation tests.
- `docs/dev/EXECUTION_LOG_summary_ocr_clean.md`: this ledger.

### Tests added

Nine tests listed above. No existing assertion was changed.

### Observed stats

On the ~2,400-characters-per-slide browser fixture: 230 input lines, 200 output lines (30 repeated chrome lines removed).

### Found but not fixed

See the section above: 5c, 5d, 5e, pre-existing `pnpm test` environment failure, and manual acceptance not run.

### Uncertainty

- Near-duplicate detection uses the chrome-filtered signature before cross-slide dedupe. This is necessary for the rule to have any effect; it is documented in the module comment.
- `lines_in` counts every raw line returned by `str::lines()`, including blank lines; `lines_out` counts only normalized surviving lines. This makes the before/after comparison the most conservative one.
- Idempotence is tested on dissimilar slides. A chain of slides that merge into one aggregate is stable on re-cleaning because the output has no consecutive neighbours left to compare; a contrived case where a later slide resembles an earlier aggregate more than its immediate neighbour was not constructed.

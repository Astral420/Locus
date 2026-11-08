# Execution log — budget the slide text and stop the fold loop (5d)

## Base

- Base branch: `dev`
- BASE_SHA: `1632bf347332124034192edfc7e15e87b21303a7`
- Branch: `fix/summary-slide-budget`
- Sync: `git fetch origin --prune && git switch dev && git pull --ff-only origin dev`.
  `dev` had moved from `9134c55` to `1632bf3`, bringing in 5a (prompt stats), 5b
  (OCR cleaning) and 5c (presentation detection). All line references below were
  re-read against `1632bf3`.
- `git status --short` at sync: clean.
- Upstream check before finishing: `git fetch origin && git log --oneline
  BASE_SHA..origin/dev` was empty, so no rebase was needed.

### Pre-existing work adapted

- `load_context` already selects slides with `ORDER BY 2, occurrence ordinal,
  s.id`, so the "random UUID order" issue from the analysis was already fixed by
  5c. No query change needed; the chronological-order test already existed
  (`slides_reach_the_prompt_in_chronological_order`).
- Slide text is built from `clean_slides` (5b); the cap is applied after cleaning
  as requested. Raw stored OCR rows are untouched.
- `PromptStats` already has `slide_chars_available`, `fold_calls_initial`,
  `fold_calls_extra` and `fold_loop_exhausted` (5a). The new loop reuses them.

## Phases

### Phase 0 — baseline

Commands and results:

- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 417 passed, 3 ignored.
- `pnpm build`: PASS.
- `pnpm test`: **FAIL (pre-existing)** — 84 failed test files / 149 failed tests,
  failing on `localStorage.clear()` being undefined (`src/tests/redesign_tweaks.test.tsx`
  and others). This is an existing environment/setup failure unrelated to this
  backend task; `src/` was not touched. Re-run after the change gave the identical
  84/149 counts.

### Phase 1 — constants + `cap_slide_text`

Files changed: `src-tauri/src/llm/summarization.rs`.

- Added `SLIDE_TEXT_MAX_CHARS = 1_800` (about 20% of `MAX_SINGLE_CONTEXT_CHARS`)
  and `SLIDE_TEXT_MIN_PER_SLIDE_CHARS = 150`, next to the existing context
  constants, with comments.
- Added `render_slide`, `cap_slide_text` and `truncate_at_line_boundary`.
  `cap_slide_text` keeps chronological order and ids/timestamps, returns the
  unchanged join when the text already fits, otherwise gives each kept slide an
  equal share and cuts at line boundaries. The number of kept slides is the
  largest count that can still honour the per-slide minimum including the
  separating newlines, so later slides are dropped rather than every slide being
  truncated to a sliver.
- `generate_payload` now builds `slide_text` with `cap_slide_text` for
  presentations (screen captures still send an empty string). The raw string is
  still counted for `slide_chars_available`.

### Phase 2 — fold loop

Files changed: `src-tauri/src/llm/summarization.rs`.

- Replaced the `while` with a `loop` that exits when:
  (a) `notes + capped_slide <= MAX_SINGLE_CONTEXT_CHARS`,
  (b) `passes == MAX_FOLD_PASSES`,
  (c) a pass did not reduce the notes (`folded.chars().count() >= before`), or
  (d) the notes already fit in one `HIERARCHICAL_CHUNK_CHARS` chunk (another fold
  cannot guarantee a smaller prompt).
- (b) and (c) set `stats.fold_loop_exhausted` and emit
  `locus: summary fold loop stopped without fitting (notes=<n> slides=<n>)`, then
  still send the request. (d) is effectively unreachable with the cap in place
  (notes <= 4,000 plus slides <= 1,800 is under 9,000); it is kept as a guard.
- Existing constant values and `INTERMEDIATE_MAX_TOKENS` were not changed.

### Phase 3 — tests

Files changed: `src-tauri/src/llm/summarization.rs`.

Added:

- `slide_cap_is_a_no_op_when_the_text_already_fits`
- `slide_cap_budgets_a_large_deck_and_keeps_it_ordered`
- `slide_cap_never_cuts_through_a_multibyte_character` (Japanese + accented)
- `oversized_slide_text_is_capped_and_does_not_exhaust_the_fold_loop`
- `huge_slide_text_does_not_add_fold_passes_to_a_long_transcript`
- `fold_loop_terminates_when_notes_cannot_shrink` (echo provider)
- Helpers: `large_presentation_deck` (>12,181 raw chars, 12 slides, under the
  1,200 avg density so it classifies as a presentation), `add_large_presentation_deck`,
  `set_transcript_text`, `long_transcript`, `cleaned_slide`; added
  `FormatCaptureProvider::fold_calls`; added `EchoProvider`.

### Phase 4 — ledger

Files changed: `docs/dev/EXECUTION_LOG_summary_slide_budget.md` (this file).

## Commands after the change

- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS (one pre-existing
  `private_interfaces` warning in `slide_text.rs`).
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 422 passed, 3 ignored
  (417 baseline + 6 new, minus the rewritten obsolete test which was replaced
  one-for-one; 5 net new tests).
- `rustfmt --edition 2021 --check src-tauri/src/llm/summarization.rs`: PASS
  (whole-repo `cargo fmt --check` still reports pre-existing diffs in other
  files).
- `pnpm build`: PASS.
- `pnpm test`: FAIL, same pre-existing 84 files / 149 tests; unchanged from
  baseline.

## Existing test that had to change

`oversized_slide_text_exhausts_the_fold_loop` asserted the old bug: that a
12k-character slide deck forces the hierarchical path, exhausts the fold loop
and spends extra passes. That is precisely the behaviour this task removes. It
was rewritten as
`oversized_slide_text_is_capped_and_does_not_exhaust_the_fold_loop`, reusing the
same deck (extended by one topic to exceed the measured 12,181 characters) and
asserting the new contract: single pass, `slide_chars <= 1,800`, no folds, not
exhausted. No other existing test was edited.

## Fold calls on the measured 12,181-character fixture

- Before (old logic): short transcript + 12,181 slide chars → 1 initial fold +
  4 extra passes (the `while` runs to `MAX_FOLD_PASSES` because the slides alone
  exceed 9,000 and never shrink) = **5 fold calls**. Long transcript (18,300
  chars) + same slides → 5 initial + 4 extra = **9 fold calls**.
- After: short transcript → **0 fold calls** (single pass, slides capped to
  <=1,800). Long transcript → **5 fold calls** (5 initial chunks, 0 extra; the
  fixture's transcript is 18,300 characters).

## Decisions

- The cap is applied to the cleaned slides (5b), matching the existing prompt
  path; the raw `slide_chars_available` statistic still reports the pre-cap size.
- When the minimum share cannot be honoured for every slide, the earliest
  chronological slides are kept and later ones are dropped. This preserves the
  beginning of the meeting and keeps ordering, at the cost of the tail.
- A slide whose first line alone exceeds its share contributes nothing (cutting
  mid-line is forbidden). In practice shares are >=150 characters and slide
  lines are shorter.
- (d) does not set `fold_loop_exhausted`; spec item 5 only assigns that flag to
  exits (b) and (c). With capped slide text (d) cannot trigger anyway.

## Found but not fixed

- `slide_text::clean_slides` is `pub` while its `SlideRow` argument is
  `pub(crate)`, producing a `private_interfaces` warning. Pre-existing; left as
  is to keep scope tight.
- `pnpm test` is red at baseline (missing `localStorage` in the test
  environment). Out of scope (frontend / test setup) and explicitly not to be
  touched here.
- To test the cap end-to-end the fixture must be classified as a presentation.
  The measured real meeting (5 slides, 12,181 chars) is classified as a screen
  capture by 5c, so its slide text is dropped before the cap ever matters. The
  cap mainly protects the "presentation but text-heavy" case, which the new
  fixture models. Noted for the acceptance run.
- Whole-repo `cargo fmt --check` reports formatting diffs in unrelated files
  (e.g. `catalog/download.rs`). Not touched.

## Open questions / uncertainty

- The task text allowed capping raw `[id @ ts]` entries when no cleaning step
  exists. Cleaning exists (5b), so the cleaned path is capped; the raw fallback
  was not implemented because it would be dead code.
- Dropping the latest slides rather than distributing the budget across the
  whole timeline is a judgement call; the spec only said "drop slides that do
  not fit". No user-visible requirement settled this.

## Final report

- Base SHA: `1632bf347332124034192edfc7e15e87b21303a7`
- Final code SHA: `83d7600603e097f6f1c079e5bf3e0179b13c4b6b` (this ledger follows in
  its own docs commit, mirroring the other `fix/summary-*` branches).
- `dev` moved during the task: no (`BASE_SHA..origin/dev` empty); no rebase.
- Files changed: `src-tauri/src/llm/summarization.rs`,
  `docs/dev/EXECUTION_LOG_summary_slide_budget.md`.
- Tests added: 6 (5 unit/integration net new plus the rewritten obsolete test).
- Existing assertion changed: `oversized_slide_text_exhausts_the_fold_loop`
  rewritten (reason above).
- Fold calls on the 12,181-character fixture: before 5 (short) / 9 (long),
  after 0 / 5.
- Suites: cargo test PASS (422), cargo build PASS, rustfmt PASS, pnpm build
  PASS, pnpm test FAIL pre-existing and unchanged.

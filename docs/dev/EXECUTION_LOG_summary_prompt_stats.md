# Execution log: record summary prompt section sizes (5a)

## Sync

- Base SHA: `fec2610cfc857a98cc5b459dd31afeca91f77899` (`dev`).
- Branch: `fix/summary-prompt-stats`.
- `git status --short` before work: clean.
- `dev` had moved from the analysis SHA `9134c55` to `fec2610` (diarization work). `git diff 9134c55..HEAD -- src-tauri/src/llm/summarization.rs` was empty, so the analysis still applied.
- No rebase was needed afterwards; `origin/dev` did not move during the task.

## Findings from re-reading the current code

- `ChatCompletionResponse` already carries `prompt_tokens: Option<u64>` (`llm/mod.rs`).
- `llama_cpp.rs` already reads `usage.prompt_tokens`, and OpenAI, Anthropic, Gemini and Ollama already fill it too. No request or provider change was needed.
- `FailureRecord` and `render_failure_record` already existed from 4a; the new stats reuse that sink for failure and `LOCUS_SUMMARY_DEBUG` files.
- The rest of the analysis held: single-pass threshold `MAX_SINGLE_CONTEXT_CHARS`, four extra fold passes, slide text never shortened, and slide text placed after the transcript.

## Baseline (before changes)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 387 passed, 0 failed, 3 ignored |
| `pnpm test` | FAIL (pre-existing/environmental): Vitest collects vendored `packaging/llama-engine/.work/.../llama.cpp/tools/ui` tests and `localStorage` is undefined. 84 files / 149 tests failed, 10 files / 111 tests passed. |
| `pnpm build` | PASS: `tsc --noEmit` and `vite build` completed. |

## Phase 1: prompt stats and diagnostics

Files changed:

- `src-tauri/src/llm/summarization.rs`
- `docs/dev/EXECUTION_LOG_summary_prompt_stats.md`

Decisions:

- Added a private `PromptStats` filled inside `generate_payload`: `path`, `system_chars`, `primary_chars`, `slide_chars`, `slide_chars_available`, `slides_available`, `transcript_chars_original`, `fold_calls_initial`, `fold_calls_extra`, `fold_loop_exhausted`, `prompt_tokens`.
- `system_chars` is the final call's system prompt length. `primary_chars` is the transcript on the single path and the folded notes on the hierarchical path. `slide_chars` is the formatted slide text actually sent; `slide_chars_available` is the same text before any later filtering.
- `fold_calls_initial` counts the first hierarchical chunks; `fold_calls_extra` counts the loop passes. `fold_loop_exhausted` is true only when `MAX_FOLD_PASSES` was reached and the prompt still did not fit.
- `generate_payload` now returns `(GeneratedSummary, PromptStats)`; the caller ignores the stats. The stats are returned so tests can assert on them directly without reading stderr.
- One stderr line per summary attempt: `locus: summary prompt: path=... system=... primary=... slides=<sent>/<available> folds=<initial>+<extra> exhausted=<bool> prompt_tokens=<n|?>`. It is also printed when a provider call fails, with whatever counts were known.
- `FailureRecord` gained `prompt_stats: Option<String>`. Failure files and `LOCUS_SUMMARY_DEBUG` files render it as `prompt stats: <same fields> transcript_original=<n> slides_available=<n>`.
- Stats contain counts only; no transcript or OCR text is copied into them.
- No prompt, budget, slide handling, schema or provider request body changed.

Tests added in `summarization.rs`:

- `oversized_slide_text_exhausts_the_fold_loop`: slide text above the single-pass limit reports `path=hierarchical`, `fold_loop_exhausted=true`, and `fold_calls_extra <= MAX_FOLD_PASSES`.
- `a_small_meeting_takes_the_single_path_without_folds`: `path=single`, zero initial and extra folds.
- `prompt_stats_contain_counts_but_no_transcript_content`: a distinctive transcript marker never appears in the stats line.
- `failure_diagnostics_carry_prompt_stats`: a parse failure writes `prompt stats:` into the failure file.
- `successful_summary_diagnostics_carry_prompt_stats`: the record path used by `LOCUS_SUMMARY_DEBUG` writes `prompt stats:` and the prompt token count.

Existing tests changed: none. The only mechanical edit was adding `prompt_stats: None` to the existing `record_with` test helper so `FailureRecord` keeps compiling.

## Phase 1 verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 392 passed, 0 failed, 3 ignored (baseline 387; 5 new tests) |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing `localStorage`/vendored-test environment issue as baseline) |
| `git diff --check` | PASS |

## Found but not fixed

- `pnpm test` is broken on this machine independently of this change: Vitest includes vendored llama.cpp UI tests outside the app, and `localStorage` is undefined under the current Node environment. Not touched because it is outside this task and the frontend must not change.
- The slides query still uses `ORDER BY id`, and `slides.id` is a random UUID, so OCR text still reaches the model in random order. 5a is visibility only; this belongs to the later slide-ordering fix.
- The fold loop can still exhaust all passes and send a prompt above `MAX_SINGLE_CONTEXT_CHARS` when OCR is huge. This is now visible as `exhausted=true`; the budget fix is 5d.
- The diagnostic stats line includes `transcript_original` and `slides_available` in addition to the mandated stderr fields, to keep the two recorded fields useful and avoid dead code.
- Manual acceptance (the user's meeting with 5 slides / 12,181 OCR characters) was not run here; no local model run was performed.

## Final report

### SHAs and sync

- Base SHA: `fec2610cfc857a98cc5b459dd31afeca91f77899`.
- Implementation commit SHA: `fb695a7`.
- Final branch tip: the docs commit that records this report.
- `dev` had moved from the analysis SHA `9134c55` to `fec2610` before work started. `summarization.rs` was unchanged between them, so no adaptation was needed. `origin/dev` did not move afterwards and no rebase was required.

### Files changed per phase

Phase 1 (stats and diagnostics):

- `src-tauri/src/llm/summarization.rs`: `PromptStats`, stats collection in `generate_payload`, stderr line, `FailureRecord::prompt_stats`, diagnostic rendering, five tests, and the `generate_payload` tuple return.
- `docs/dev/EXECUTION_LOG_summary_prompt_stats.md`: this ledger.

### Tests added

- `oversized_slide_text_exhausts_the_fold_loop`
- `a_small_meeting_takes_the_single_path_without_folds`
- `prompt_stats_contain_counts_but_no_transcript_content`
- `failure_diagnostics_carry_prompt_stats`
- `successful_summary_diagnostics_carry_prompt_stats`

No existing assertion was changed.

### Found but not fixed

- `pnpm test` remains red on this machine for a pre-existing environment reason (vendored llama.cpp UI tests collected; `localStorage` undefined). It was red at baseline before any edit.
- Slide text still reaches the model in random UUID order (`ORDER BY id`), and the fold loop can still exhaust with oversized OCR. Both are visible now and are the subject of later fixes.
- Manual acceptance was not run here.

### Uncertainty

- `slide_chars_available` equals `slide_chars` today because nothing filters slides yet. The field exists so 5d can report the difference without changing the stats shape.
- `fold_loop_exhausted` is computed after the loop from `passes >= MAX_FOLD_PASSES` plus the still-over-limit condition, so it stays correct if 5d changes the loop bound or the slide budget.

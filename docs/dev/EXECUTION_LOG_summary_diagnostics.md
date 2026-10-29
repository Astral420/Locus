# Execution log: summary failure diagnostics (4a)

## Base and final revisions

- BASE_SHA: `7913982d15f5af4ca4b6493ad5fd1c7715485b9d`
- FINAL_SHA (implementation): `58f2281858164cd97d1544b0176144def21f1677`
- Branch: `fix/summary-diagnostics`
- `dev` did not move during execution (`7913982..origin/dev` was empty). No rebase was needed.

## Phase 0: sync and baseline

Commands:

- `git fetch origin --prune && git switch dev && git pull --ff-only origin dev` — pass (already at `7913982`).
- `git status --short` — clean before starting.
- `cargo test --manifest-path src-tauri/Cargo.toml` — pass, 347 passed, 2 ignored.
- `pnpm build` — pass (tsc + vite).
- `pnpm test` — fail, 149 failed / 111 passed (260 total), 84 failed test files. Pre-existing: `localStorage` is undefined in the happy-dom setup (`src/tests/redesign_tweaks.test.tsx:28` and many other suites). Not caused by this change; the frontend was not modified.

## Phase 1: field path, failure sink, wiring, tests

Files changed:

- `src-tauri/Cargo.toml` — added `serde_path_to_error = "0.1.20"`.
- `src-tauri/Cargo.lock` — committed the new dependency.
- `src-tauri/src/llm/summarization.rs` — structured `SummaryParseError { message, path, raw }`, `serde_path_to_error` based parsing, `FailureRecord`, `SummaryFailureSink`, `NoopSummaryFailureSink`, `FileSummaryFailureSink`, debug capture, completion-token capture, new tests.
- `src-tauri/src/lib.rs` — injects `FileSummaryFailureSink` at `<data_dir>/logs/summary-failures`.

Behavior:

- Parse failures now read like `summary schema validation failed at key_concepts[0]: invalid type: map, expected a string at line N column M`.
- `generate_payload` still returns `LlmError::InvalidResponse(message)` to callers, appending `Raw model output saved to <path>.` only when the file was written.
- One stderr line on failure: `locus: summary parse failed (<file path or field path>)`.
- Diagnostic files keep the newest 5 `.txt` files, are capped at 64 KiB, and use a sanitized UTC timestamp plus meeting id as the file name.
- `LOCUS_SUMMARY_DEBUG=1` also captures successful raw output, in the same folder with the same retention. Default off.
- Sink I/O errors are ignored; the original parse error is returned unchanged.

Tests added (all in `llm/summarization.rs`):

- `a_parse_failure_names_the_field_and_records_the_raw_output`
- `only_the_newest_five_failure_files_are_kept`
- `diagnostic_content_is_truncated_at_64_kib`
- `an_unwritable_failure_directory_leaves_the_original_error_intact`
- `a_completion_at_the_token_limit_is_flagged_in_the_diagnostic_file`
- `a_successful_summary_writes_nothing_unless_debug_capture_is_enabled`

No existing test assertions were changed.

Verification after the phase:

- `cargo test --manifest-path src-tauri/Cargo.toml` — pass, 353 passed, 2 ignored.
- `pnpm build` — pass.
- `pnpm test` — same pre-existing 149 failures/111 passes, unchanged.
- `cargo clippy --all-targets` — no warnings in `summarization.rs`; remaining warnings are pre-existing elsewhere.
- `rustfmt --check` on the touched Rust files — only a pre-existing formatting difference in an untouched existing test line remains; the new code is formatted.

## Decisions

- Folder is exactly `<data_dir>/logs/summary-failures/`, retention 5, 64 KiB cap, as specified.
- File names sanitize `:` and other unsafe characters in the RFC 3339 timestamp; this keeps the timestamp + meeting-id shape while staying portable. Collisions within the same nanosecond get a numeric suffix.
- The 64 KiB cap applies to the entire diagnostic text, so the header is preserved and the raw output is what gets cut. This keeps the file itself at or below 64 KiB.
- Successful debug captures reuse the same sink and folder with the message `summary parsed successfully`.
- `complete` now returns the whole `ChatCompletionResponse` so the final pass's `completion_tokens` is available for the truncation warning.

## Found but not fixed

- `pnpm test` has a pre-existing environment failure where `localStorage` is undefined under happy-dom (149 failing tests). It is unrelated to this backend-only change.
- `cargo fmt --check` reports pre-existing formatting differences in untouched files (for example `src-tauri/src/catalog/download.rs`) and one pre-existing assertion in `summarization.rs`'s older tests. Not reformatted to avoid unrelated churn.
- Root-cause acceptance behavior is unchanged by design: object-shaped `key_concepts`, dates, etc. still fail validation. That is addressed by the sibling 4b/4c/4d tasks.

## Open questions / uncertainty

- Manual UI verification was not run here (requires a real failing provider call in the desktop app). The pipeline stores `StepOutcome::Failed` and `trigger_summary` returns the error string without truncation, so the appended file path should reach the UI.
- Timestamp collision handling uses a numeric suffix after the timestamp; retention sorts by file name, which preserves chronological order for normal timestamps.

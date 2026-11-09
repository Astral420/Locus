# Execution log: summary prompt source priority (5e)

## Sync

- Analysis SHA: `9134c55`.
- Synced `dev` before work: `git fetch origin --prune`, `git switch dev`, `git pull --ff-only origin dev`.
- Base SHA: `5774d401fe3ff38ac64eb3cd1a6612c29050384f` (`dev`, includes 5a diagnostics, 5b OCR cleaning, 5c presentation detection and 5d slide budget).
- Branch: `fix/summary-source-priority-prompt`.
- `git status --short` before work: clean.
- `dev` had moved past `9134c55`; re-read `prompts.rs`, `summarization.rs`, the slides query and the prompt tests against current code. `prompts.rs` was unchanged by 5a-5d; `summarization.rs` changed substantially.
- Already fixed upstream: the slides context query now orders chronologically (`ORDER BY 2, (first occurrence ordinal), s.id`) instead of by random UUID. No change needed here.

## Baseline (before changes)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 422 passed, 0 failed, 3 ignored |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (pre-existing/environmental): Vitest collects vendored llama.cpp UI tests and `localStorage` is undefined. 84 files / 149 tests failed, 10 files / 111 tests passed. |

## Phase 1: source priority rule, user prompt order/labels, identity

Files changed:

- `src-tauri/src/llm/prompts.rs`
- `src-tauri/src/llm/summarization.rs`
- `docs/dev/EXECUTION_LOG_summary_source_priority.md`

Decisions:

- Added one shared `SOURCE_PRIORITY_RULE` (three sentences) after each kind instruction and before the schema spec. Every system prompt inherits it.
- Added `PrimarySource::{Transcript, Notes}` and changed `user_prompt(kind, source, primary_text, slide_text)`. The slide section is emitted only when slide text is non-empty and comes before the primary section; the instruction line is last.
- `complete` now takes the source kind; the single pass passes `Transcript`, the hierarchical final pass passes `Notes`.
- Bumped `PromptKind::identity()` to `business.v3` / `lecture.v3` / `generic.v3`.

## Phase 1 verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 427 passed, 0 failed, 3 ignored (baseline 422; 5 new tests) |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing/environmental `localStorage` failure as baseline): 84 files / 149 tests failed, 10 files / 111 tests passed |
| `git diff --check` | PASS |

Composed system prompt lengths (bound `2_400`, unchanged):

- business: 2,358
- lecture: 2,280
- generic: 2,332

The source-priority rule fits without shortening anything else or raising the bound.

## Tests added

`src-tauri/src/llm/prompts.rs`:

- `every_system_prompt_states_the_source_priority_rule`
- `user_prompt_puts_slide_text_before_the_primary_source`
- `user_prompt_omits_the_slide_section_when_there_is_no_slide_text`
- `user_prompt_labels_notes_as_the_primary_source_on_the_hierarchical_path`

`src-tauri/src/llm/summarization.rs`:

- `the_hierarchical_final_pass_labels_folded_notes_as_the_primary_source` (drives `generate_payload` through the hierarchical path and inspects captured messages)

Existing assertions changed: none. No existing test asserted the old `(none)` wording or the old section order.

## Final report

### SHAs and sync

- Analysis SHA: `9134c55`.
- Base SHA: `5774d401fe3ff38ac64eb3cd1a6612c29050384f`.
- `dev` had moved from `9134c55` to `5774d40` before work (5a-5d merged). Re-read the current files and adapted; no rebase was needed because `origin/dev` stayed at `5774d40` through the task.
- Implementation commit: `51e204fb76ae3ea1bf771ca6c8d7c641bb35b4aa`.
- Final branch tip: the docs commit that records this report.

### Files changed per phase

- Phase 1: `src-tauri/src/llm/prompts.rs`, `src-tauri/src/llm/summarization.rs`, `docs/dev/EXECUTION_LOG_summary_source_priority.md`.

### Behaviour

- All three system prompts now include one shared three-sentence source-priority rule: speech/notes are primary and authoritative, slide text is supplementary on-screen text for clarifying names/terms/figures, never describe UI chrome as meeting content or treat slide text as speech, and the transcript wins disagreements.
- `user_prompt` now takes `PrimarySource::{Transcript, Notes}`. The slide section appears only when slide text is non-empty, comes before the primary section, and the JSON instruction is last. The primary heading is truthful on the hierarchical path (`Notes from the transcript`).
- Both call paths pass the right source kind; no schema, folding prompt, request setting or budget/loop logic changed.
- `PromptKind::identity()` is now `business.v3` / `lecture.v3` / `generic.v3`.
- Upstream 5d already orders slide rows chronologically, so no query change was needed.

### `prompt_identity` consumers found

- `src-tauri/src/llm/summarization.rs`: `FailureRecord.prompt_identity`, `SummaryRevisionDto.prompt_identity`, revision SELECT/INSERT, failure record construction, `publish` and `record_summary_output`.
- `src/lib/tauri.ts`: `prompt_identity: string | null` type declaration only.
- No backend or frontend code parses or compares it against the bare kind name. `PromptKind::meeting_type()` remains the bare kind-derived value. `knowledge.rs` calls a different provider `identity()` and is unrelated.

### Found but not fixed

- `pnpm test` is broken on this machine independently of this change: Vitest collects vendored llama.cpp UI tests outside the app and `localStorage` is undefined in the current Node environment.
- `SummarizationService::complete` still names its primary-text parameter `transcript` even when it receives folded notes. Harmless and internal; left untouched to keep the diff minimal.
- Manual acceptance (a genuine presentation, plus the screen-capture meeting regenerated with Gemma and Qwen) is left to the user; no local model run was performed here.

### Manual/developer check: forcing slide text

No runtime switch forces slide text into the prompt; 5c drops it when `classify_slide_source` says `ScreenCapture`. For the developer-only check, in a scratch working tree temporarily make `generate_payload` skip the `SlideSourceKind::ScreenCapture` branch (use `cap_slide_text(&cleaned.slides, SLIDE_TEXT_MAX_CHARS)` unconditionally), regenerate the screen-capture meeting once with Gemma and once with Qwen, confirm both still anchor on the speech and do not describe the screen, then revert the temporary patch. Do not commit that patch.

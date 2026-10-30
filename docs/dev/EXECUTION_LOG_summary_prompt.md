# Execution log: tighten summarization prompts (4b)

## Sync

- Base SHA: `8ea14cdc519a6e666485041475aab024f2ae09cf` (`dev`, includes 4a diagnostics from PR #15).
- Branch: `fix/summary-prompt`.
- `git status --short` before work: clean.
- `dev` moved from the analysis SHA `7913982`; re-read `prompts.rs` and `summarization.rs` against current code.
- 4a landed before this task. `prompts.rs` was unchanged by 4a; `summarization.rs` gained diagnostics.

## Baseline (before changes)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 353 passed, 0 failed, 2 ignored |
| `pnpm test` | FAIL (pre-existing/environmental): Vitest collects vendored `packaging/llama-engine/.work/.../llama.cpp/tools/ui` tests and `localStorage` is undefined. 84 files / 149 tests failed, 10 files / 111 tests passed. |
| `pnpm exec vitest run src/tests` | FAIL (same environmental cause): `localStorage` is undefined; 149 failed, 110 passed. Not caused by this change. |
| `pnpm build` | PASS: `tsc --noEmit` and `vite build` completed. |

## Phase 1: prompt schema block, example, identity

Files changed:

- `src-tauri/src/llm/prompts.rs`
- `src-tauri/src/llm/summarization.rs`
- `docs/dev/EXECUTION_LOG_summary_prompt.md`

Decisions:

- Added one shared `SUMMARY_SCHEMA_SPEC` with the exact JSON shape, plain-string rule, null/empty rules, and one compact example. Each kind's system prompt is `instruction + schema spec` via `LazyLock<String>`.
- Kept each kind's intent and "never invent" wording.
- Added `PromptKind::identity()` returning `business.v2` / `lecture.v2` / `generic.v2`; `publish` and the failure diagnostic record use it. `meeting_type` is untouched.
- Added `PromptRouter::folding_system_prompt()` so long-transcript folding asks for free-text notes instead of the JSON summary prompt (removes the contradiction with the new rules).
- User prompt now ends with `Return only the JSON object described in the system prompt.`

Tests added in `prompts.rs`:

- embedded example deserializes into the real `GeneratedSummary` and key types are asserted;
- every prompt mentions every `GeneratedSummary` field, the one-object rule and the plain-strings rule;
- length bound (`MAX_SYSTEM_PROMPT_CHARS = 2_400`);
- prompts differ per kind.

Existing tests changed: none.

Findings for consumers of `prompt_identity`:

- `src-tauri/src/llm/summarization.rs`: `FailureRecord`, `SummaryRevisionDto`, SQL SELECT/INSERT, failure record construction and publish.
- `src/lib/tauri.ts`: type declaration only.
- No backend or frontend code parses or compares it against the bare kind name. `meeting_type` remains the bare kind-derived value.

## Phase 1 verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 357 passed, 0 failed, 2 ignored (baseline 353; 4 new tests) |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing `localStorage`/vendored-test environment issue as baseline) |

System prompt character lengths (bound `2_400`):

- business: 1,883
- lecture: 1,805
- generic: 1,857

## Found but not fixed

- `pnpm test` is broken on this machine independently of this change: Vitest includes vendored llama.cpp UI tests outside the app, and `localStorage` is undefined under the current Node environment. Not touched because it is outside this task.
- 4c (tolerant parsing) and 4d (constrained decoding) are still required to guarantee valid output; this prompt change only reduces the chance of object-in-string-list output.
- Manual acceptance with `gemma-4-12B-it-qat-q4_0` (3 runs on a short meeting plus the two previously failing meetings) is left to the user; no local model run was performed here.

## Final report

### SHAs and sync

- Base SHA: `8ea14cdc519a6e666485041475aab024f2ae09cf`.
- `dev` had moved from the analysis SHA `7913982` to `8ea14cd` before work started. Re-read the current files; no rebase was needed afterwards because `origin/dev` did not move during the task.
- Implementation commit SHA: `2fed5252a078a0f720760f26ccd80cd962a382c3`.
- Final branch tip: the docs commit that records the implementation SHA.

### Files changed per phase

Phase 1 (prompts and identity):

- `src-tauri/src/llm/prompts.rs`: shared `SUMMARY_SCHEMA_SPEC` with example, per-kind instructions composed through `LazyLock`, `PromptKind::identity()`, `folding_system_prompt()`, user-prompt ending, tests.
- `src-tauri/src/llm/summarization.rs`: fold pass uses the free-text folding prompt; `publish` and the failure record use the versioned identity.
- `docs/dev/EXECUTION_LOG_summary_prompt.md`: this ledger.

### Tests added

- `embedded_example_deserializes_into_generated_summary`
- `every_system_prompt_names_the_summary_fields_and_the_string_rule`
- `composed_system_prompts_stay_within_the_instruction_budget`
- `system_prompts_differ_by_kind`

No existing assertions were changed.

### Prompt identity consumers

- `src-tauri/src/llm/summarization.rs`: failure record, revision DTO, SELECT/INSERT columns, publish, and diagnostic record construction.
- `src/lib/tauri.ts`: TypeScript type declaration only.
- Nothing parses or compares `prompt_identity` against the bare kind name. `meeting_type` is unchanged.

### Found but not fixed

- `pnpm test` remains red on this machine for a pre-existing environment reason (vendored llama.cpp UI tests collected; `localStorage` undefined). It was red at baseline before any edit.
- Manual model acceptance was not run here. The user still needs to summarize the two previously failing meetings plus three short-meeting runs with `gemma-4-12B-it-qat-q4_0`.
- 4c and 4d remain necessary for guaranteed valid output.

### Uncertainty

- The failure was reproduced only indirectly in the original analysis; this change reduces object-in-string-list output but cannot prove the Gemma model will comply on every run without the manual acceptance step.
- The folding prompt change is limited to removing the JSON-vs-notes contradiction; the folding user text and logic are otherwise untouched.

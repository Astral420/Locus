# Execution log: constrained JSON output for llama-server (4d)

## Sync

- `BASE_SHA`: `2b60dd72e8f0da98d2074546ef6d6a1357d672a2` (`dev` HEAD at start)
- Branch: `fix/summary-constrained-json` (created from `dev`)
- `git pull --ff-only origin dev`: already up to date; working tree clean before branching.
- `dev` had moved past the analysis SHA `7913982`, and 4a/4b/4c are merged:
  - `58f2281` 4a diagnostics, `55ca4b4` ledger, merge #15
  - `2fed525` 4b prompt, `9cd497e` ledger, merge #16
  - `8e5c59a` 4c tolerant parsing, `5d7ed91` ledger, merge #17
- `git diff --stat 7913982..HEAD -- src-tauri/src/llm knowledge.rs packaging/llama-engine`:
  only `llm/prompts.rs` (+141) and `llm/summarization.rs` (+1111). Nothing in
  `llm/mod.rs`, `llm/llama_cpp.rs` or the engine packaging had changed, so every line
  reference in the task was re-checked and still valid.
- Because 4b and 4c are merged, the schema was derived from `GeneratedSummary` itself and
  cross-checked against the field list 4b spells out in `prompts.rs`
  (`SUMMARY_SCHEMA_SPEC`); the two agree.

## Phase 0 - baseline (before any edit)

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 369 passed, 0 failed, 2 ignored |
| `pnpm build` | PASS (`tsc --noEmit` + vite; chunk-size warning only) |
| `pnpm test` | FAIL (pre-existing): 84 files / 149 tests red. Node 26 reports `localStorage` as unavailable, and vitest also collects the git-ignored `packaging/llama-engine/.work` llama.cpp UI tree (`TSConfckParseError`). Identical counts before and after this change. |
| `cargo clippy --all-targets` | PASS with pre-existing warnings, all outside the files touched here (16 warnings: `capture/frame.rs`, `capture/windows.rs`, `pipeline/*`, ...) |
| `cargo fmt -- --check` | Not clean on `dev` (24 files with pre-existing diffs); changed lines are formatted, see Verification |

## Phase 1 - implementation

Files changed:

- `src-tauri/src/llm/mod.rs`: `ResponseFormat` enum and `ChatCompletionRequest.response_format`.
- `src-tauri/src/llm/summarization.rs`: `summary_json_schema()`, schema on the final pass only, tests.
- `src-tauri/src/llm/llama_cpp.rs`: request body builder, `response_format` forwarded, one-shot retry, tests.
- `src-tauri/src/knowledge.rs`: request built by a documented helper with `response_format: None`, test.
- `src-tauri/src/llm/generation_server.rs`: `response_format: None` at the two test construction sites.
- `src-tauri/src/llm/{openai,anthropic,gemini,ollama}.rs`: one comment each naming the native mechanism to wire later.
- `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`: `jsonschema = "0.60.2"` as a dev-dependency only.

What changed:

1. **Request type.** `ResponseFormat::JsonSchema { name, schema }` (`#[serde(tag = "type")]`) and
   `response_format: Option<ResponseFormat>` on `ChatCompletionRequest`, skipped when serialising
   `None`. Every existing construction site sets `None`, so wire behaviour is unchanged for them.
2. **Schema.** `summary_json_schema()` mirrors `GeneratedSummary`: `title` string-or-null,
   `overview` string, `decisions` / `key_concepts` / `assignments` / `important_dates` /
   `disclosures` arrays of **strings**, `action_items[]` with `text` plus four nullable strings and
   `evidence_refs` (array of strings), `citations[]` with `kind` / `ref_id` strings,
   `start_seconds` / `end_seconds` number-or-null, `page_number` integer-or-null,
   `additionalProperties: false` on every object and **every property in `required`**. No `$ref`,
   `not`, `if/then` or pattern properties, so the pinned converter handles it (verified below).
3. **Only the final summary call is constrained.** `SummarizationService::complete` sends
   `Some(JsonSchema { name: "meeting_summary", ... })`. The folding closure and
   `knowledge_chat_request` send `None`.
4. **llama-server provider.** `chat_request_body` adds
   `response_format: {"type":"json_schema","json_schema":{"name":...,"strict":true,"schema":...}}`
   only when the request asks for it.
5. **Compatibility fallback.** A 400 or 422 answer to a request that carried a schema is retried
   **once** without the key, after
   `locus: llama-server rejected response_format; retrying without schema`. Timeouts, crashes and
   every other status are never retried (`rejects_response_format`).
6. **Other providers.** Comments only: OpenAI `response_format: json_schema`, Anthropic tool use
   with `input_schema`, Gemini `responseSchema`, Ollama `format`. They ignore the field, no warnings.
7. **Truncation note (change 8).** No edit: 4a is merged and `lib.rs` installs
   `FileSummaryFailureSink` (line ~110), so a summary cut off at `max_tokens` already produces
   `WARNING: output may have been truncated at max_tokens` in the saved diagnostic and the error
   message names that file. `SUMMARY_MAX_TOKENS` and the context constants were not touched.

## Tests added (9)

- `llm::summarization::the_summary_request_carries_the_schema`
- `llm::summarization::folding_passes_stay_free_text_and_only_the_final_pass_is_constrained`
- `llm::summarization::the_schema_accepts_a_full_and_a_quiet_summary`
- `llm::summarization::the_schema_rejects_the_structured_lists_from_the_bug_report`
- `llm::summarization::print_summary_schema` (ignored; manual helper, follows the existing
  `print_launch_spec_for_real_machine` precedent, used for the engine check below)
- `llm::llama_cpp::the_request_body_carries_response_format_only_when_asked`
- `llm::llama_cpp::only_client_errors_are_treated_as_a_rejected_schema`
- `llm::llama_cpp::a_schema_request_sends_response_format_and_a_plain_one_does_not` (python fake
  server that records every request body)
- `llm::llama_cpp::a_rejected_response_format_is_retried_exactly_once_without_it` (python fake
  server that answers 400 to any request carrying `response_format`; asserts two requests, the
  second without the key and with identical messages)
- `knowledge::tests::knowledge_chat_requests_are_not_schema_constrained`

No existing assertion was changed. The only edits inside existing tests are the mechanical
`response_format: None` field additions where a `ChatCompletionRequest` is built.

Known limitation of the Knowledge assertion: a real `send_message` only reaches the completion
provider when the knowledge index has chunks, which needs an embedding provider and the Python
sidecar. The request is therefore built by `knowledge_chat_request(...)` and asserted directly
instead of through a full chat round trip.

## Engine verification

- Pinned engine: `packaging/llama-engine/pin.env` -> ToshLLM `v0.87.26`
  (`523b814ac8cc61d80afbc73cece581c34234658b`), llama.cpp
  `d81235049384534c167caea52b85a694f6103d14`. The clone in `packaging/llama-engine/.work`
  reports exactly that llama.cpp SHA.
- `packaging/llama-engine/dist/llama-server` is present (26,923,352 bytes, built 22 Oct).
- The engine side of the wire format was read in the pinned source:
  `tools/server/server-common.cpp:1299-1315` reads `response_format`, takes
  `json_schema.schema` for `type: "json_schema"` (and `schema` for `json_object`), and
  `common/chat.cpp:1455` converts it with `json_schema_to_grammar` when `--jinja` is off, which is
  the launch spec this app uses. Unknown `response_format.type` values throw, which is what the
  one-shot retry covers.
- **Live sampling was not possible**: no generation-capable GGUF exists on this machine (only
  tokenizer-only `models/ggml-vocab-*.gguf` files in the vendored tree), so `llama-server` cannot
  load weights and no completion could be sampled. This was not faked.
- Instead the schema was converted by the pinned engine's own converter. Built the
  `test-json-schema-to-grammar` target from the pinned source in a throwaway
  `build-schema-check/` directory, then linked a two-line program against `libllama-common` that
  calls `json_schema_to_grammar` on the exact schema the app sends
  (`cargo test print_summary_schema -- --ignored --nocapture`). Conversion exited 0 and produced a
  fixed-shape grammar:

  ```
  root ::= "{" space title-kv "," space overview-kv "," space action-items-kv "," space
           decisions-kv "," space key-concepts-kv "," space assignments-kv "," space
           important-dates-kv "," space citations-kv "," space disclosures-kv space "}"
  decisions ::= "[" space (string ("," space string)*)? space "]"
  key-concepts ::= "[" space (string ("," space string)*)? space "]"
  citations-item-page-number ::= integer | null
  title ::= string | null
  ```

  i.e. every key is mandatory and in struct order, the five list fields can only contain strings,
  and the nullable fields accept `null`. The throwaway build directory was deleted afterwards.

## Verification after the change

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS: 378 passed, 0 failed, 3 ignored (369 + 9 new) |
| `pnpm build` | PASS |
| `pnpm test` | FAIL (same pre-existing 84 files / 149 tests; counts unchanged) |
| `cargo clippy --all-targets` | No warning in any touched file; 16 pre-existing warnings elsewhere, unchanged |
| `cargo fmt -- --check` | Changed lines clean; the remaining diffs in `llama_cpp.rs` / `knowledge.rs` / `generation_server.rs` are pre-existing lines |
| Engine converter check | PASS (see above) |
| `git log --oneline BASE_SHA..origin/dev` | Empty: `dev` did not move, no rebase needed |

## Decisions

- Schema lives in `summarization.rs` next to `GeneratedSummary` rather than in a new
  `llm/summary_schema.rs`: one fewer module for a function only summarization uses.
- Retry is decided by status only, before the body is parsed, so the successful path keeps using
  `parse_json` unchanged.
- `"strict": true` and `"name"` are sent for OpenAI compatibility; the pinned engine ignores both
  and reads only `schema` (verified in the source above).
- The schema requires every property, so the grammar has exactly one shape. Empty arrays and
  `null` stay valid, which is what 4b's prompt already tells the model to emit.

## Found but not fixed

- `pnpm test` is red on `dev` before this change (Node 26 `localStorage` plus vitest collecting the
  ignored engine vendor tree). Unrelated to this task, counts unchanged.
- `cargo fmt` and clippy drift across ~24 files, all pre-existing.
- Remote providers are still unconstrained. The same class of failure remains possible there until
  the native mechanisms noted in their comments are wired; out of scope for 4d.
- Grammar sampling gives no upper bound on list lengths, so a runaway model can still exhaust
  `max_tokens` mid-JSON. That failure stays a parse error with 4a's truncation warning. The token
  budget was deliberately not raised here.
- The retry fires on any 400/422 for a schema request, including a 400 caused by something else
  (for example malformed messages). The second attempt then surfaces the server's real error, so
  this costs one extra request in a rare path rather than hiding anything.

## Final report

- Base SHA: `2b60dd72e8f0da98d2074546ef6d6a1357d672a2`
- Final implementation SHA: `2503da6` (this ledger is the follow-up docs commit, matching the 4a-4c
  pattern of one implementation commit plus a ledger commit)
- `dev` moved during the task? No; `origin/dev` never advanced past the base, so no rebase.
- Files changed: `src-tauri/src/llm/{mod,summarization,llama_cpp,generation_server,openai,anthropic,gemini,ollama}.rs`,
  `src-tauri/src/knowledge.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, plus this ledger.
- Tests added: the nine listed above; no existing assertion changed.
- Engine verified against: pinned ToshLLM `v0.87.26` / llama.cpp `d812350` — wire format read in
  source and the exact schema converted to a valid grammar with that commit's converter. Live
  sampling was not possible (no generation GGUF on the machine).
- Found but not fixed: the five items above.
- Unsure / open: whether grammar sampling noticeably slows a real Gemma run on the target Intel Mac
  (only a manual run can answer that), and whether `strict: true` / `name` in the wrapper stay
  harmless across future engine pins — the retry covers a rejection either way.

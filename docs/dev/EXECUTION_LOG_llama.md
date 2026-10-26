# Execution log: local llama generation for summarization and chat

Task: EXEC-2-llama-summarization-chat. Branch `fix/llama-local-generation`.

## Phase 0 — sync and baseline

- `git fetch origin --prune`, `git switch dev`, `git pull --ff-only origin dev`: HEAD was already `a0e1ff6`.
- **BASE_SHA**: `a0e1ff62221a88f8928cb7b2e6441567f845afe7` (= `origin/dev` at start).
- `git status --short` was **not** clean: ` M .codebuffignore`. That file is the harness's own read-ignore list
  (it adds `!EXEC-2-llama-summarization-chat.md`). It is a tooling artifact, not project source; it is left
  uncommitted and untouched, and no commit in this run includes it. No other working-tree changes existed.
- Prompt analysis was done at `5ee777d`. `5ee777d` is in history. `git diff --stat 5ee777d..HEAD` for the
  paths this task touches is **empty**; the only upstream change since is
  `packaging/llama-engine/source-offer.sh` + its test (release tooling). The prompt's file/line references
  were therefore re-checked but no re-mapping was needed.
- Branch created: `fix/llama-local-generation` from `dev`.

### Baseline suites (recorded before any edit)

| Suite | Command | Result |
| --- | --- | --- |
| Rust | `cargo test --manifest-path src-tauri/Cargo.toml` | **326 passed, 0 failed, 2 ignored** — green. (The five `capture::*` FFmpeg tests pass on this machine; a full-featured FFmpeg is installed.) |
| Frontend | `pnpm test` | **108 passed / 149 failed, 84 of 93 files failed** — red at baseline, environmental. See below. |
| Frontend (project tests only) | `NODE_OPTIONS=--localstorage-file=/tmp/locus-ls.json pnpm vitest run src/tests` | **254 passed / 2 failed** |
| Frontend build | `pnpm build` | success |
| Sidecar | `python3 -m pytest tests` (in `sidecar/`) | **19 passed, 3 skipped** |

Why `pnpm test` is red at baseline, in two independent parts:

1. **Node 26 `localStorage`.** Node 26 defines a global `localStorage` that is *undefined* unless node is
   started with `--localstorage-file`. That global shadows happy-dom's working implementation, so every test
   that calls `localStorage.clear()` or reads persisted UI state throws
   `TypeError: Cannot read properties of undefined (reading 'clear')`. This is an environment/toolchain issue,
   not a test or code defect, and it is out of scope here: it is **not fixed**.
2. **Vendored llama.cpp UI tests are collected.** `packaging/llama-engine/.work/toshllm/vendor/llama.cpp/tools/ui/tests/**`
   exists on this machine (the engine was built locally) and matches vitest's default include globs; those 77
   files fail to collect (SvelteKit tsconfig missing). Also out of scope: not fixed.

All before/after frontend comparisons in this log therefore use
`NODE_OPTIONS=--localstorage-file=/tmp/locus-ls.json pnpm vitest run src/tests`, whose baseline is
**2 failures**: `redesign_shell.test.tsx > sidebar > hides completely when collapsed … persists the choice`
and `redesign_tweaks.test.tsx > Offline-Ready Core Engine notice … shows on first visit`. Both exercise
persistence and are artifacts of the Node `localStorage` shim, not of this task.

## Findings re-verified against current code

All six A.1 findings hold on `a0e1ff6`, with two corrections:

- `GenerationModelManager` records generation assets with `role='generation'` (not `role='llm'`), and
  `select_model` maps UI role `"llm"` to the `llm.generation_selection` setting. The new
  `GenerationModelManager::selected()` therefore queries `role='generation'`, not `role='llm'`.
- `EmbeddingServerPlan`/`plan_embedding_server` live in `commands/knowledge.rs` (not in `llama_cpp.rs`), as
  the prompt says; they move to `llm/llama_cpp.rs` with `pub(crate) use` aliases kept in place.

## Phase A1 — resolve the engine once (`735f684`)

Files: `src-tauri/src/llm/engine.rs`, `src-tauri/src/llm/backend.rs`, `src-tauri/src/commands/knowledge.rs`.

- `resolve_llama_server()` now memoises through `OnceLock<ResolvedEngine>` (`resolved_engine()`). Candidate
  order: `LOCUS_LLAMA_SERVER_BIN`, bundled `<exe>/../Resources/llama-engine/`, then `llama-engine/` (relative to
  cwd), `src-tauri/llama-engine/`, and — under `cfg(debug_assertions)` —
  `CARGO_MANIFEST_DIR/llama-engine/` and `CARGO_MANIFEST_DIR/../packaging/llama-engine/dist/`; `PATH` last.
  The extra debug candidates are what make `tauri dev` find the locally built patched engine.
- One log line, printed once by the memoised resolver. When the result is `Source=Path` with no patched
  manifest, a second line prints `current_dir()` and the candidates that were tried, so the fallback is
  explainable instead of mysterious.
- `backend.rs`: `detect_llama_backend()` uses `resolved_engine()`; new `LlamaRuntimeInfo` + `llama_runtime_info()`
  expose backend, device, reason, engine source/path, and the patched manifest (tag + llama.cpp commit).
  `choose_llama_backend`, `parse_list_devices`, `is_amd_gpu` and their tests are untouched.
- `commands/knowledge.rs::embedding_provider()` launches from `resolved_engine().binary`, so the second
  "llama backend" log line is gone.

New tests: `checkout_candidates_cover_the_dev_directory_and_the_packaging_drop`.

Commands: `cargo test --manifest-path src-tauri/Cargo.toml` → pass. On this machine the real resolver picks
`packaging/llama-engine/dist/llama-server` and `detect_llama_backend()` answers `MetalAmd[0]`
(`AMD Radeon RX 6600`), verified by the detection test with the real binary present.

## Phase A2 — a real local generation server (`2e41e67`)

Files: `llm/model_manager.rs`, `llm/llama_cpp.rs`, `llm/generation_server.rs` (new), `llm/mod.rs`,
`commands/models.rs`, `commands/summarization.rs`, `commands/knowledge.rs`, `pipeline/steps.rs`,
`knowledge.rs`, `lib.rs`.

- `GenerationModelManager::selected()` mirrors `EmbeddingModelManager::selected()`: reads
  `llm.generation_selection`, loads the `model_assets` row (`role='generation'`, `state='ready'`), resolves the
  relative path under the managed generation folder, falls back to the single ready asset, and errors with
  "No local language model is selected. Download one in Models, then press Set active."
- `llama_cpp.rs`: `ServerPlan`/`plan_server` moved here (`pub(crate)` aliases keep the old names in
  `commands/knowledge.rs` and its `embedding_server_plan_tests` compile unchanged); `LlamaServerConfig.log_path`
  appends child stdout/stderr to a file instead of `Stdio::null()`, and startup failures append the last 20 log
  lines (never the API token); `complete()` posts to `/v1/chat/completions` with the request's messages,
  temperature and max_tokens and parses `choices[0].message.content` + `usage`; `-c <context>` is passed only to
  generation servers, `-ngl 99` is added for `AppleMetal` generation.
- `generation_server.rs` keeps one reusable `LlamaServerManager` per selection: a healthy server launched with
  the wanted weights is reused, a changed selection relaunches, and the previous server stops when its last
  provider is dropped. `build_runtime_provider` returns `LocalGenerationProvider` for `ProviderKind::LlamaServer`
  (engine log at `<data_dir>/logs/llama-generation.log`) and delegates every other kind to `build_provider`
  unchanged.
- `AppState.generation_server` added; `trigger_summary`, `retry_summary` and `send_knowledge_message` are now
  `async` and run on `spawn_blocking` (first Metal run can take minutes; synchronous commands run on the main
  thread). JS call sites are unchanged.
- `knowledge.rs::send_message` inserts an assistant message in state `error`
  ("I couldn't generate an answer: …") before returning the failure, so the thread shows what happened.

New tests (fake python server, skipped when `python3` is missing): reuse of the running server,
relaunch on model switch, clear error with nothing selected, old server stopped, chat-completions endpoint +
forwarded sampling options, engine-log capture, startup failure quoting the log (and not the token), `-c` only on
generation, `-ngl 99` on Apple Metal, plus the three `selected()` cases.

Commands: `cargo check --all-targets` → pass; `cargo test` → pass.

## Phase A2b — summaries inside the engine window (`5850462`)

File: `src-tauri/src/llm/summarization.rs`.

- Chunking moved to `char_indices` boundaries (`char_chunks`), so a multi-byte character at a chunk edge is no
  longer turned into U+FFFD by `from_utf8_lossy`.
- Sizes now fit `-c 8192`: `MAX_SINGLE_CONTEXT_CHARS = 9_000` (≈3 000 tokens), `SUMMARY_MAX_TOKENS = 4_096`, i.e.
  ~1 000 tokens of headroom for instructions and the chat template. Hierarchical passages are 4 000 chars with
  700-token notes, and folded notes are folded again (bounded passes) until they fit, so the final pass cannot
  overflow either. Raising the window to 16 384 was rejected: it needs a bigger KV cache on every machine
  (3B model on an 8 GB AMD card) for a summary that is folded anyway.

New tests: character-boundary splitting without replacement characters, the single-pass limit leaving room for
its completion, and a 48k-character transcript whose every request fits the window.

Commands: `cargo test --manifest-path src-tauri/Cargo.toml summarization` → pass (after one wrong expectation of
my own was corrected: the folded-notes test needed the pass count of the bounded folding, not one pass).

## Phase A3 — real provider selection, no silent mock data (`a9401ff`)

Files: `src/lib/tauri.ts`, `src/routes/SettingsView.tsx`, `src/routes/KnowledgeView.tsx`,
`src/tests/llama_wiring.test.tsx` (new).

- `safeInvoke`→`invokeStrict` inside Tauri for `send_knowledge_message`, `trigger_summary`,
  `create_knowledge_thread`, `upload_document`, `list_knowledge_messages`, `list_knowledge_threads`,
  `list_documents`, `select_provider`, `configure_provider`, `save_provider_api_key` (plus the earlier
  `list_models`/`select_model`). Outside Tauri the mock fallbacks are unchanged, so the browser preview and the
  vitest suite behave as before.
- New `getSelectedProvider()` (`get_selected_provider`): the backend's "no rows" answer becomes `null` (nothing
  selected yet); every other error rejects. New `getLlamaBackend()` for `get_llama_backend`.
- Settings: the selection is read from the persisted snapshot — no `useState("llama_server")` default and no
  auto-selection anywhere. Llama.cpp's Select calls
  `selectProvider("llama_server", <active generation model id | "local">, "http://127.0.0.1 (local, offline)", true)`
  and reports the outcome in `providerMessage` (failure reverts the optimistic state). Remote providers keep their
  existing disclosure + keychain form.
- "Inference Backend" under Llama.cpp now renders `getLlamaBackend()` (backend, device + index, engine source/path,
  "Patched engine" badge, llama.cpp commit). The whisper/Vulkan probe moved, relabelled, to "Transcription Backend
  (whisper.cpp)".
- Knowledge: a rejected send is shown in `role="alert"`; the "No local language model is selected" message links to
  Models.

New test: `src/tests/llama_wiring.test.tsx` — with a rejecting `invoke`, chat and summary reject with the backend
message (never the canned sentence, one attempt, no fallback), and Select calls `selectProvider` with the active
local model, loopback destination and `accept_disclosure: true`.

Commands: `tsc --noEmit` → pass; `pnpm build` → pass; vitest project suite → pass. Note: A4's Knowledge
vector-store warning lives in this commit too, because it edits the same file as A3's alert.

## Phase A4 — observability (`ac654f4`)

Files: `src-tauri/src/knowledge.rs`, `src-tauri/src/sidecar/client.rs` (+ the TS DTO and KnowledgeView warning
from A3).

- `commands/models.rs::get_llama_backend` (`LlamaRuntimeInfo`) is registered in `lib.rs` (landed with A2 because
  `lib.rs` was already being edited there).
- `IndexStatusDto.vector_backend` comes from the sidecar's `health()` (`vector_store`). `health` is only asked
  when the sidecar is already running (`SidecarManager::health_if_running`): the Knowledge page polls
  `get_knowledge_index_status` every three seconds and describing the process must not spawn one. The answer is
  cached for the service lifetime (ChromaDB availability cannot change while the process lives), so the field is
  `"unknown"` until the first index/search starts the sidecar. `store.py` is unchanged.
- Knowledge shows a non-blocking warning when the value is `"deterministic-fallback"`.

New test: `index_status_never_starts_the_sidecar_to_describe_its_vector_store`.

Commands: `cargo test` → pass; project vitest suite → pass.

## Suites after all phases

| Suite | Command | Before | After |
| --- | --- | --- | --- |
| Rust | `cargo test --manifest-path src-tauri/Cargo.toml` | 326 passed / 0 failed / 2 ignored | **343 passed / 0 failed / 2 ignored** (+17) |
| Frontend (project) | `NODE_OPTIONS=--localstorage-file=… vitest run src/tests` | 254 passed / 2 failed | **258 passed / 1 failed** (the second baseline failure, the sidebar-persistence test, is flaky and passed on this run; the remaining one is the same Node `localStorage` shim issue) |
| Frontend build | `pnpm build` | pass | pass |
| Sidecar | `python3 -m pytest tests` | 19 passed / 3 skipped | 19 passed / 3 skipped |

No existing assertion was changed in Rust or in the frontend suite.

## Real-machine evidence (this Mac, Intel x86_64 + AMD RX 6600)

- `packaging/llama-engine/dist/llama-server --list-devices` →
  `ggml_metal: device 0: AMD Radeon RX 6600 …` and `MTL0: AMD Radeon RX 6600 (8176 MiB …)`; the engine manifest
  (`engine.json`) carries `toshllm_tag v0.87.26` / `llama_commit d812350…`.
- With that binary present, the Rust detection test on this machine resolves it and answers `MetalAmd[0]`
  (`AMD Radeon RX 6600`), i.e. the patched-engine path and the log line now agree with reality.
- **`pnpm tauri dev` was NOT run**, and no summary/chat was produced on this machine: there is no generation
  GGUF on disk (no `Locus` app-data folder, no `.gguf` under the project besides llama.cpp's vocab fixtures), so
  the end-to-end download → Set active → summary → `logs/llama-generation.log` path is unverified here. The
  generation server itself is exercised against a fake llama-server Python process in the Rust tests.

## Final report

1. **Base SHA** `a0e1ff62221a88f8928cb7b2e6441567f845afe7`; **final SHA** `ac654f4b638977051a80b36822ef770e6d966692`
   on `fix/llama-local-generation`. `origin/dev` did not move during the run (`git fetch && git log
   BASE_SHA..origin/dev` is empty), so no rebase was needed.
2. **Commits**:
   - `735f684` A1 — engine resolution + runtime info (`engine.rs`, `backend.rs`, `commands/knowledge.rs`).
   - `2e41e67` A2 — local generation server (`llm/*`, `commands/{models,summarization,knowledge}.rs`,
     `pipeline/steps.rs`, `knowledge.rs`, `lib.rs`; new `llm/generation_server.rs`).
   - `5850462` A2b — summary chunking/limits (`llm/summarization.rs`).
   - `a9401ff` A3 — provider selection + strict IPC + Settings/Knowledge UI (`src/lib/tauri.ts`,
     `src/routes/*`, new `src/tests/llama_wiring.test.tsx`).
   - `ac654f4` A4 — vector backend in index status (`knowledge.rs`, `sidecar/client.rs`).
   - Tests added: 17 Rust tests, 3 frontend tests. Suites before/after are in the table above.
3. **Existing assertions changed**: none.
4. **`safeInvoke(..., fallback)` left in place** (each can still hide a real-app failure behind mock data and is
   outside this task's "knowledge, summary, provider and model groups"): `list_meetings`, `get_meeting`,
   `get_meeting_media`, `delete_meeting`, `get_pipeline_status`, `retry_pipeline_step`, `list_transcript_segments`,
   `rename_speaker`, `get_slides`, `list_summary_revisions`, `list_action_items`, `toggle_action_item`,
   `get_gpu_backend`, `get_storage_info`, `list_provider_configs`, `check_for_update`, `refresh_model_catalog`,
   `cancel_model_download`, `skip_embedding_setup`, `retry_knowledge_index`, `greet`, `get_recording_state`,
   `show_recording_window`. Calls that already have no fallback (`download_model`, `uninstall_model`,
   `migrate_models`, `relocate_storage`, `set_meeting_*`, `retry_summary`) surface as they should.
5. **App run with the patched engine**: **not run** (`pnpm tauri dev` was not started; see the evidence section for
   what was verified instead, and why: no local generation weights).
6. **Findings that differed from the prompt**: `GenerationModelManager` uses `role='generation'`, not `role='llm'`
   (the prompt's A.3 wording), and `EmbeddingServerPlan` lives in `commands/knowledge.rs` rather than
   `llama_cpp.rs`. Everything else in A.1 held on the fresh `dev`. Nothing in the task had already been fixed
   upstream.
7. **Found but not fixed**:
   - `pnpm test` is red for two environmental reasons unrelated to this task: Node 26's global `localStorage`
     shadows happy-dom's, and vitest collects the vendored `packaging/llama-engine/.work/…/tools/ui/tests` when the
     engine was built locally.
   - The Knowledge "Destination" dropdown is still a static provider list that is not tied to the persisted
     selection (it never sends anything, but it can show a destination that is not the selected provider).
   - `get_gpu_backend` keeps its browser-preview fallback: inside the app a failed whisper probe renders the
     "Browser preview has no native GPU runtime" text. Left as found, since whisper is out of scope.
   - No GGUF generation model is installed on this machine, so a real summary/chat was not produced.

---

# Execution log: EXEC-2b — local summarization request timeout

Prompt: `EXEC-2b` (local summarization fails with `error sending request for url
(http://127.0.0.1:<port>/v1/chat/completions)`). Branch `fix/llama-request-timeout`
from `fix/llama-local-generation`.

## Phase 0 — sync and baseline

- `git fetch origin --prune`, `git switch fix/llama-local-generation`, `git pull --ff-only`:
  HEAD was already `138b3e445933f7fe24b5982091f737a97b26afe2`, the analysis SHA.
- **BASE_SHA**: `138b3e445933f7fe24b5982091f737a97b26afe2`.
- `git status --short` was clean; branch created: `git switch -c fix/llama-request-timeout`.
- `origin/fix/llama-local-generation` did not move during the run
  (`git log 138b3e4..origin/fix/llama-local-generation` is empty), so no rebase.
- Root cause confirmed in the current code: `LlamaServerManager::launch` used `Client::new()`
  (reqwest 0.12.28 blocking), whose default total request timeout is 30 s; `complete()` inherited it,
  and `LlmError::Request(error.to_string())` hid the timeout/closed-connection cause.

## Phase 1 — client timeouts (`llama_cpp.rs`)

- `Client::new()` replaced by `build_client()`: `connect_timeout(5 s)`, total `timeout(None)` (the
  blocking 30 s default is gone), default pool idle timeout.
- `GENERATION_REQUEST_TIMEOUT = 30 * 60` (named `pub const`) is applied per request by
  `complete()` and `complete_prompt()`, and stored on the manager so tests can shorten it with a
  `#[cfg(test)] set_generation_timeout`.
- Health checks now carry their own `HEALTH_CHECK_TIMEOUT = 10 s`; embedding requests carry
  `EMBEDDING_REQUEST_TIMEOUT = 60 s` so a wedged embedding server cannot block indexing.
- No automatic retry of a generation request.

## Phase 2 — errors that name the cause (`llama_cpp.rs`)

- `describe_request_error()` walks `std::error::Error::source()` and joins the chain, prefixing a
  classification: `timed out` (`is_timeout`), `could not connect` (`is_connect`), `connection closed`
  (including a body/decode variant). Used by `health()`, `embed()` and the generation paths.
- `request_failure()` checks the child with `try_wait()` (300 ms grace so a just-crashed process is
  reaped deterministically). If it exited:
  `the local language model stopped unexpectedly (exit status: N)` plus the last 20 lines of
  `LlamaServerConfig.log_path` (the token is a CLI argument and never appears in the log). If it is
  still running: the classified message plus
  `The model server is still running; the request may have been too slow or too large. See <log>.`
- `log_path` is now kept on the manager for that message. A dead server makes `health()` fail, which is
  exactly what the generation slot already uses to relaunch.

## Phase 3 — observable slow generation

- `pipeline/steps.rs::summarization` records 1 % progress before the blocking generation, so the step
  reads as running instead of staying at 0 % for minutes.
- `commands/summarization.rs` already returns `error.to_string()` unchanged (including the new
  classification); no truncation or wrapper was added.
- No streaming in this task. Recommendation: show a "Generating summary…" running label in
  `PipelineStatusBadge` and surface a rejected manual `triggerSummary` in `SummaryTab` (currently a
  rejected regenerate is swallowed by `finally`); both are frontend work and per `AGENTS.md` are
  routed to the frontend model, so they were not edited here.

## Phase 4 — tests (4 added, no existing test changed)

- `generation_requests_have_no_thirty_second_cap` — contract: `GENERATION_REQUEST_TIMEOUT` is 30 min,
  not the reqwest default.
- `a_slow_generation_is_reported_as_timed_out_with_the_log_path` — fake python server answers after
  1 s; manager allowance shortened to 150 ms; asserts `timed out`, the log path and the still-running
  hint.
- `a_server_that_dies_mid_request_reports_its_exit_status_and_log_tail` — fake server `os._exit(9)`
  mid-request; asserts `stopped unexpectedly`, `exit status: 9`, the log path and the logged Metal line.
- `a_process_that_died_mid_request_is_relaunched_on_the_next_request` (`generation_server.rs`) — the
  first request reports the crash; the next `generation_server_with` sees failing health, relaunches
  (launches.log goes 1 -> 2) and answers normally.

## Suites (final revision)

| Suite | Command | Result |
| --- | --- | --- |
| Rust | `cargo test --manifest-path src-tauri/Cargo.toml` | **347 passed / 0 failed / 2 ignored** (baseline 343) |
| Frontend | `pnpm test` | **149 failed / 111 passed** — same environmental baseline: Node 26 `localStorage` and vendored llama.cpp UI tests (see the first log), not caused by this change |
| Frontend (project) | `NODE_OPTIONS=--localstorage-file=/tmp/locus-ls.json pnpm vitest run src/tests` | **259 passed / 0 failed** |
| Frontend build | `pnpm build` | success |
| Formatting | `rustfmt --check` on the three touched files | only pre-existing formatting diffs (catalog/download.rs and older hunks) |

No manual real-model run on this machine: no generation GGUF is installed here, so the AMD Metal
end-to-end case remains for the user's Intel Mac. If it still fails, the new message distinguishes
`timed out` from `the local language model stopped unexpectedly (exit status: …)` + log tail; a
`ggml_metal` abort would be a separate engine task.

## Final report (EXEC-2b)

1. **Base SHA** `138b3e445933f7fe24b5982091f737a97b26afe2`; **final code SHA** `f73551e`
   on `fix/llama-request-timeout` (log commit follows). `origin/fix/llama-local-generation` did not
   move; no rebase.
2. **Commits**:
   - `c621233` Phase 1 — client connect timeout 5 s, no total timeout, generation allowance 30 min,
     health 10 s, embedding 60 s (`llm/llama_cpp.rs`).
   - `3ae3c24` Phase 2 — cause-chain classification, exit-status + log-tail reporting, slow/crash
     tests (`llm/llama_cpp.rs`).
   - `c790b76` Phase 3 — summarization step records 1 % running progress (`pipeline/steps.rs`).
   - `f73551e` Phase 4 — relaunch-after-mid-request-death test (`llm/generation_server.rs`).
3. **Root cause (one line)**: `Client::new()` gave every blocking llama-server request reqwest's
   default 30 s total timeout, so a real multi-minute summary aborted with the generic
   "error sending request for url (…/v1/chat/completions)" message.
4. **Files changed**: `src-tauri/src/llm/llama_cpp.rs`, `src-tauri/src/llm/generation_server.rs`,
   `src-tauri/src/pipeline/steps.rs`, this log.
5. **Tests added**: 4 (constant contract, slow-answer timeout, mid-request crash, slot relaunch after
   death). **Existing assertions changed**: none.
6. **Suites (final revision)**: Rust `347 passed / 0 failed / 2 ignored`; project vitest
   `259 passed / 0 failed`; `pnpm build` success; `pnpm test` still red only for the two documented
   environmental reasons (Node 26 `localStorage`, vendored llama.cpp UI tests).
7. **Not fixed / found**:
   - No real GGUF run here, so the user's Gemma 4 Metal AMD case is unverified. The new message will
     now say whether it was a timeout or a process death (with log tail); a `ggml_metal` abort would
     be a separate engine task.
   - Frontend: `SummaryTab` does not display a rejected manual `triggerSummary`, and
     `PipelineStatusBadge` shows the step kind ("Summarization (1%)") rather than a
     "Generating summary…" sentence. Both are frontend changes (routed to the frontend model per
     `AGENTS.md`), noted as the next UI follow-up; no streaming was added.
   - `cargo fmt --check` is not clean repository-wide (pre-existing formatting in
     `catalog/download.rs` and older hunks); the touched files were not reformatted wholesale to
     avoid unrelated churn.

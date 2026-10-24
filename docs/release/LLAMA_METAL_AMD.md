# Intel Mac + AMD llama.cpp (Metal) qualification

The patched ToshLLM `llama-server` is the llama.cpp engine for Intel Macs with an
AMD dGPU. It runs on Metal and enables the AMD attention kernels with
`TOSH_FA_AMD=1`. This document defines the correctness gates and records the
evidence from a real machine. **Correctness gates block a release; speed is
recorded, never gated.** No invented latency threshold applies.

The Vulkan/MoltenVK document (`VULKAN_MACOS_INTEL.md`) covers **whisper only**.
Its llama-server instructions are historical and are not this path.

## How to run

Build the engine and install the two lab models (`00-README.md`,
`packaging/llama-engine/README.md`).

L1, any machine (no model or GPU):

```sh
python3 -m py_compile packaging/llama-engine/qualify.py
python3 packaging/llama-engine/qualify.py --self-test
```

`--self-test` runs every gate against a built-in fake server and then checks that
deliberately broken fakes (empty replies, garbage text, wrong dimension,
drifting vectors, hung process) fail the matching gate.

L5, Intel Mac (human):

```sh
python3 packaging/llama-engine/qualify.py \
  --binary packaging/llama-engine/dist/llama-server \
  --chat-model  ~/locus-engine-lab/gemma-3-1b-it-Q4_K_M.gguf \
  --embed-model ~/locus-engine-lab/minilm-q8_0.gguf --embed-dim 384 \
  --gpu-name "RX 6600" \
  --bench packaging/llama-engine/.work/toshllm/vendor/llama.cpp/build-static/bin/llama-bench \
  --out ~/locus-engine-lab/report-$(hostname -s).md
```

The harness picks a free port, sends each server's stderr to a temp log, and
always stops the child. GPU runs use `TOSH_FA_AMD=1 -ngl 99 -fa auto -c 4096`;
CPU runs use `-ngl 0`; embedding servers also get `--embedding`. Paste the
generated report into **Results** below. Do not edit a gate threshold to make a
run pass.

## Gates

| Gate | Check | FAIL when |
|---|---|---|
| Q1 start | `/health` OK within 150 s; log mentions `--gpu-name` | no health; a missing name is `WARN` |
| Q2 chat sane | 5 fixed prompts, `temperature 0`, `max_tokens 24` on GPU | any empty, invalid UTF-8, fewer than 3 distinct alphabetic words, or > 60 % of words the same word |
| Q3 chat vs CPU | same prompts on CPU; first 6 words compared | fewer than 3 of 5 prompts match |
| Q4 embeddings | 20 fixed sentences on CPU; dimension equals `--embed-dim`; 5 similar/unrelated triples rank correctly | wrong dimension or any triple misranked |
| Q5 embeddings GPU vs CPU | same sentences with `-ngl 99 -fa auto`; cosine per sentence | any cosine < 0.999 |
| Q6 soak | 50 sequential chats + 200 embeddings on GPU | any error; p95 latency > 3x median is `WARN` |
| Q7 stop | send SIGTERM; process gone within 5 s | still alive |
| Bench (optional) | `llama-bench -m <chat> -p 512 -n 128 -ngl 99` and `-ngl 0` | never fails; tokens/s recorded |

Q5 decides whether GPU embeddings may ever be enabled by default. Until Q5
passes on real hardware, embeddings stay on the CPU path.

## Results

One row per L5 run. Gate cells are `PASS` or `FAIL`; bench cells are tokens/s.
A human runs the command above on the Intel Mac and pastes the generated report
here. Missing metadata stays `not provided`; a failing gate is recorded, never
waived.

| Run | Machine | macOS | GPU | Engine pin | Date | Q1 | Q2 | Q3 | Q4 | Q5 | Q6 | Q7 | Bench t/s |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| L5 run 1 | not provided | not provided | RX 6600 | not provided | not provided | PASS | FAIL | PASS | PASS | PASS | FAIL | PASS | GPU pp512 3569.82; GPU tg128 91.57; CPU pp512 444.43; CPU tg128 58.25 |

Q2 and Q6 failed on the same fixed prompt, `What is 2 + 2?`: a correct answer
such as `4` contains no alphabetic words, so the "fewer than 3 distinct
alphabetic words" check fails. Q6 reuses the same prompt for chat 5. The failure
is recorded as-is. After this run the fifth prompt was replaced with
`Describe a rainy morning in one sentence.` so a concise correct answer cannot
trip the sanity check; no gate threshold changed. L5 must be rerun against the
updated fixture. Q5 passed narrowly at cosine 0.999722.

No row means **unverified**.

## Optional experiments

Record these; do not adopt them without a separate decision:

- Repeat Q2 with `TOSH_FA_AMD` unset to see what the flash-attention switch
  changes.
- Try `--load-mode none` only if the pinned engine's `--help` documents it.

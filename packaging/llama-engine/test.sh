#!/usr/bin/env bash
# Runs ToshLLM's engine suites against the built llama-server. No model or GPU needed.
# Extra arguments go to unittest, e.g. packaging/llama-engine/test.sh -v.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
engine_root="$root/packaging/llama-engine"
tosh="$engine_root/.work/toshllm"
bin="$engine_root/dist/llama-server"
runner="$tosh/scripts/test-engine.sh"

[ -d "$tosh/.git" ] || { echo "error: ToshLLM is not checked out; run packaging/llama-engine/build.sh first" >&2; exit 2; }
[ -x "$bin" ] || { echo "error: no engine at $bin; run packaging/llama-engine/build.sh first" >&2; exit 2; }
[ -f "$runner" ] || { echo "error: missing $runner" >&2; exit 2; }

log="$(mktemp "${TMPDIR:-/tmp}/locus-engine-tests.XXXXXX")"
trap 'rm -f "$log"' EXIT

set +e
( cd "$tosh" && LLAMA_SERVER_BIN_PATH="$bin" ./scripts/test-engine.sh "$@" ) 2>&1 | tee "$log"
status="${PIPESTATUS[0]}"
set -e

suite_lines="$(grep -E ': ran [0-9]+, passed [0-9]+, failed [0-9]+, skipped [0-9]+$' "$log" || true)"
if [ -z "$suite_lines" ]; then
  echo "error: engine test suites did not run" >&2
  exit 2
fi

suites="$(printf '%s\n' "$suite_lines" | wc -l | tr -d ' ')"
ran="$(printf '%s\n' "$suite_lines" | sed -E 's/.*: ran ([0-9]+),.*/\1/' | awk '{n+=$1} END {print n+0}')"
failed="$(printf '%s\n' "$suite_lines" | sed -E 's/.*failed ([0-9]+),.*/\1/' | awk '{n+=$1} END {print n+0}')"
skipped="$(printf '%s\n' "$suite_lines" | sed -E 's/.*skipped ([0-9]+)$/\1/' | awk '{n+=$1} END {print n+0}')"

echo "engine tests: $suites suites, $ran ran, $failed failed, $skipped skipped"
if [ "$status" -ne 0 ]; then
  exit "$status"
fi
[ "$failed" -eq 0 ] || exit 1

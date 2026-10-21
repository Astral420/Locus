#!/usr/bin/env bash
# Builds the pinned ToshLLM-patched llama-server for macOS x86_64 + AMD GPUs.
# Output: packaging/llama-engine/dist (llama-server, kernels, engine.json, SHA256SUMS, license).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
engine_root="$root/packaging/llama-engine"
pin="$engine_root/pin.env"
work="$engine_root/.work"
tosh="$work/toshllm"
dist="$engine_root/dist"
deployment_target="14.0"

dry_run=0
clean=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --clean) clean=1 ;;
    -h|--help)
      echo "usage: packaging/llama-engine/build.sh [--dry-run] [--clean]"
      exit 0
      ;;
    *)
      echo "error: unknown argument: $arg" >&2
      echo "usage: packaging/llama-engine/build.sh [--dry-run] [--clean]" >&2
      exit 2
      ;;
  esac
done

[ -f "$pin" ] || { echo "error: missing $pin" >&2; exit 1; }
. "$pin"

for value in TOSHLLM_REPO TOSHLLM_TAG TOSHLLM_COMMIT LLAMA_COMMIT ARCH; do
  [ -n "${!value:-}" ] || { echo "error: $value is empty in $pin" >&2; exit 1; }
done

echo "ToshLLM pin: $TOSHLLM_TAG ($TOSHLLM_COMMIT)"
echo "llama.cpp pin: $LLAMA_COMMIT"
echo "arch: $ARCH"
echo "deployment target: $deployment_target"
echo "work: $work"
echo "dist: $dist"

if [ "$dry_run" = 1 ]; then
  echo "steps:"
  echo "  1. clone or fetch $TOSHLLM_REPO into $tosh"
  echo "  2. checkout $TOSHLLM_COMMIT and verify HEAD matches"
  echo "  3. remove $tosh/vendor/llama.cpp/build-static (only with --clean)"
  echo "  4. run SKIP_WHISPER=1 SKIP_IMAGE=1 SKIP_LLAMA= ARCH=$ARCH LLAMA_COMMIT=$LLAMA_COMMIT ./scripts/build-engines.sh"
  echo "  5. copy llama-server and kernels into $dist"
  echo "  6. write engine.json, SHA256SUMS and LICENSE-GPL-3.0.txt"
  echo "dry run: no clone or build performed"
  exit 0
fi

if [ "$clean" = 1 ]; then
  rm -rf "$tosh/vendor/llama.cpp/build-static"
  echo "removed stale build directory"
fi

if [ ! -d "$tosh/.git" ]; then
  rm -rf "$tosh"
  mkdir -p "$work"
  echo "cloning ToshLLM into $tosh"
  git clone --filter=blob:none "$TOSHLLM_REPO" "$tosh"
fi

head="$(git -C "$tosh" rev-parse HEAD 2>/dev/null || true)"
if [ "$head" != "$TOSHLLM_COMMIT" ]; then
  echo "fetching ToshLLM $TOSHLLM_COMMIT"
  git -C "$tosh" fetch --tags origin || true
  if ! git -C "$tosh" checkout -qf "$TOSHLLM_COMMIT" 2>/dev/null; then
    echo "error: cannot check out TOSHLLM_COMMIT $TOSHLLM_COMMIT" >&2
    exit 1
  fi
fi

head="$(git -C "$tosh" rev-parse HEAD)"
if [ "$head" != "$TOSHLLM_COMMIT" ]; then
  echo "error: ToshLLM pin mismatch" >&2
  echo "  expected $TOSHLLM_COMMIT" >&2
  echo "  got      $head" >&2
  exit 1
fi
echo "ToshLLM HEAD verified at $head"

echo "building llama-server (whisper and image engines skipped)"
(
  cd "$tosh"
  SKIP_WHISPER=1 SKIP_IMAGE=1 SKIP_LLAMA= ARCH="$ARCH" LLAMA_COMMIT="$LLAMA_COMMIT" \
    ./scripts/build-engines.sh
)

bin_dir="$tosh/vendor/llama.cpp/build-static/bin"
src="$bin_dir/llama-server"
[ -x "$src" ] || { echo "error: build did not produce $src" >&2; exit 1; }

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

rm -rf "$dist"
mkdir -p "$dist"
cp "$src" "$dist/llama-server"
chmod +x "$dist/llama-server"

has_kernels=false
if [ -f "$bin_dir/kernels/fingerprint" ]; then
  cp -R "$bin_dir/kernels" "$dist/kernels"
  has_kernels=true
fi

cp "$tosh/LICENSE" "$dist/LICENSE-GPL-3.0.txt"

llama_server_sha256="$(sha256_file "$dist/llama-server")"
built_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

cat > "$dist/engine.json" <<EOF
{
  "engine": "toshllm-llama",
  "toshllm_tag": "$TOSHLLM_TAG",
  "toshllm_commit": "$TOSHLLM_COMMIT",
  "llama_commit": "$LLAMA_COMMIT",
  "arch": "$ARCH",
  "deployment_target": "$deployment_target",
  "has_kernels": $has_kernels,
  "built_at": "$built_at",
  "llama_server_sha256": "$llama_server_sha256"
}
EOF

(
  cd "$dist"
  find . -type f ! -name SHA256SUMS -print | sort | while IFS= read -r file; do
    printf '%s  %s\n' "$(sha256_file "$file")" "$file"
  done > SHA256SUMS
)

echo "has_kernels: $has_kernels"
echo "llama-server sha256: $llama_server_sha256"
echo "engine ready at packaging/llama-engine/dist"

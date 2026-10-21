#!/usr/bin/env bash
# Stages the pinned macOS x86_64 llama-server engine into src-tauri/llama-engine/
# so the Tauri bundle ships it (bundle.resources: "llama-engine").
#
#   packaging/llama-engine/stage.sh
#   LOCUS_LLAMA_ENGINE_DIR=/path/to/dist packaging/llama-engine/stage.sh
#
# Verifies SHA256SUMS and engine.json before copying. Signs only llama-server;
# the .metallib kernels are data covered by the app's resource seal.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
src="${LOCUS_LLAMA_ENGINE_DIR:-$root/packaging/llama-engine/dist}"
dest="$root/src-tauri/llama-engine"

die() { echo "error: $*" >&2; exit 1; }

[ -d "$src" ] || die "engine directory not found: $src (run packaging/llama-engine/build.sh)"
[ -f "$src/llama-server" ] || die "missing $src/llama-server"
[ -f "$src/engine.json" ] || die "missing $src/engine.json"
[ -f "$src/SHA256SUMS" ] || die "missing $src/SHA256SUMS"
[ -d "$src/kernels" ] || die "missing $src/kernels (the engine needs kernels next to the binary)"

echo "Verifying $src/SHA256SUMS"
if command -v shasum >/dev/null 2>&1; then
  ( cd "$src" && shasum -a 256 -c SHA256SUMS )
elif command -v sha256sum >/dev/null 2>&1; then
  ( cd "$src" && sha256sum -c SHA256SUMS )
else
  die "neither shasum nor sha256sum is available"
fi

command -v python3 >/dev/null 2>&1 || die "python3 is required to read engine.json"
arch="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["arch"])' "$src/engine.json")" \
  || die "cannot parse $src/engine.json"
[ "$arch" = "x86_64" ] || die "engine.json arch is '$arch', expected 'x86_64'"

mkdir -p "$dest"
find "$dest" -mindepth 1 -maxdepth 1 ! -name README.md -exec rm -rf {} +
cp -R "$src/." "$dest/"
chmod +x "$dest/llama-server"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  command -v codesign >/dev/null 2>&1 || die "codesign is required to sign the engine"
  echo "Signing $dest/llama-server with $APPLE_SIGNING_IDENTITY"
  codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dest/llama-server"
else
  echo "note: APPLE_SIGNING_IDENTITY is not set; staged llama-server is unsigned"
fi

echo "Staged engine in $dest"

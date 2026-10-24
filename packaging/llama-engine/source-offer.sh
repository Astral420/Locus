#!/usr/bin/env bash
# Builds the corresponding-source archive published with the bundled llama-server.
#
#   packaging/llama-engine/source-offer.sh
#   LOCUS_SOURCE_OFFER_SRC=/path/to/toshllm packaging/llama-engine/source-offer.sh
#
# Output: packaging/llama-engine/dist-source/llama-engine-source-<toshllm_tag>.tar.gz
#
# The archive carries only what is needed to rebuild the engine: the ToshLLM
# patch series and scripts/build-engines.sh at the pinned commit, ToshLLM's
# LICENSE and patches/COPYRIGHT, pin.env, a generated BUILDING.md and a
# generated SOURCE-OFFER.txt. No Swift app, UI assets or vendor trees.
#
# The archive is deterministic: entries are sorted by name, every mtime is fixed
# and gzip runs without a name or timestamp, so two runs produce one sha256.
#
# LOCUS_SOURCE_OFFER_SRC names a ToshLLM checkout at any commit for local tests;
# the pinned-commit check is then skipped with a warning. A release run reads
# .work/toshllm (from build.sh) and refuses a HEAD that differs from pin.env.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
engine_root="$root/packaging/llama-engine"
pin="$engine_root/pin.env"
out_dir="$engine_root/dist-source"

die() { echo "error: $*" >&2; exit 1; }
sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

[ -f "$pin" ] || die "missing $pin"
. "$pin"

for value in TOSHLLM_REPO TOSHLLM_TAG TOSHLLM_COMMIT LLAMA_COMMIT; do
  [ -n "${!value:-}" ] || die "$value is empty in $pin"
done

llama_repo="https://github.com/ggml-org/llama.cpp"
src="${LOCUS_SOURCE_OFFER_SRC:-$engine_root/.work/toshllm}"

[ -d "$src/.git" ] || die "not a git checkout: $src (run packaging/llama-engine/build.sh, or set LOCUS_SOURCE_OFFER_SRC)"

test_checkout=0
commit="$(git -C "$src" rev-parse HEAD)"
if [ -n "${LOCUS_SOURCE_OFFER_SRC:-}" ]; then
  test_checkout=1
  echo "warning: LOCUS_SOURCE_OFFER_SRC is set; archiving $src at $commit and skipping the ToshLLM commit check (test archive, not for distribution)" >&2
elif [ "$commit" != "$TOSHLLM_COMMIT" ]; then
  die "ToshLLM HEAD is $commit, expected $TOSHLLM_COMMIT (run packaging/llama-engine/build.sh)"
fi

tosh_version="$(tr -d '[:space:]' < "$src/VERSION" 2>/dev/null || true)"
[ -n "$tosh_version" ] || tosh_version="${TOSHLLM_TAG#v}"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/locus-source-offer.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
stage="$tmp/stage"
mkdir -p "$stage"

echo "ToshLLM pin: $TOSHLLM_TAG ($TOSHLLM_COMMIT)"
echo "llama.cpp pin: $LLAMA_COMMIT"
echo "archiving $commit from $src"

git -C "$src" archive --format=tar "$commit" \
  patches scripts/build-engines.sh LICENSE \
  | tar -x -C "$stage"
cp "$pin" "$stage/pin.env"

# The rebuild recipe is generated from build.sh and scripts/build-engines.sh, not
# paraphrased: same pins, same patch order, same cmake flags, same targets.
cat > "$tmp/BUILDING.md.in" <<'EOF'
# Rebuilding the bundled llama-server

This archive is the corresponding source of the `llama-server` binary Locus ships
inside `Locus.app` on macOS x86_64 (Intel with an AMD GPU). It does not contain
llama.cpp itself; upstream is fetched at the commit pinned in `pin.env`.

- Engine: ToshLLM-patched llama.cpp server, ToshLLM @TOSHLLM_TAG@
- Patches: ToshLLM @TOSHLLM_COMMIT@ at @TOSHLLM_REPO@
- Upstream: llama.cpp @LLAMA_COMMIT@ at @LLAMA_REPO@
- Target: @ARCH@-apple-darwin, deployment target @DEPLOYMENT_TARGET@
- Licence: GPL-3.0-or-later (`LICENSE`, `patches/COPYRIGHT`)

## Toolchain

- macOS @DEPLOYMENT_TARGET@ or later, and a full Xcode install (not only the
  Command Line Tools): `xcode-select -p` must name it and
  `xcrun -sdk macosx metal --version` must print a version. If the Metal
  toolchain is missing: `xcodebuild -downloadComponent MetalToolchain`.
- `cmake` (`brew install cmake`), `python3` 3.9+, `git`, and `zsh` — ToshLLM's
  build script starts with `#!/bin/zsh`.

The commands below are the ones Locus runs, from `packaging/llama-engine/build.sh`
and the `scripts/build-engines.sh` in this archive. Work in an empty directory.

## 1. Unpack and restore the version file

```sh
tar xzf llama-engine-source-@TOSHLLM_TAG@.tar.gz
printf '%s\n' @TOSH_VERSION@ > VERSION
```

`scripts/build-engines.sh` stamps this into the binary as `TOSH_VERSION`; the
value is ToshLLM's `VERSION` at the pinned commit.

## 2. Fetch llama.cpp at the pinned commit

```sh
LLAMA_COMMIT=@LLAMA_COMMIT@
git clone --filter=blob:none @LLAMA_REPO@ vendor/llama.cpp
git -C vendor/llama.cpp fetch origin "$LLAMA_COMMIT"
git -C vendor/llama.cpp checkout -qf "$LLAMA_COMMIT"
git -C vendor/llama.cpp clean -qfd
```

## 3. Build

```sh
SKIP_WHISPER=1 SKIP_IMAGE=1 SKIP_LLAMA= ARCH=@ARCH@ LLAMA_COMMIT=@LLAMA_COMMIT@ \
  ./scripts/build-engines.sh
```

Whisper and the image engine are skipped: only `llama-server` is shipped. The
script then:

- applies every `patches/llama/**/*.patch` with `git apply`, ordered by the
  numeric prefix alone, so the numbers are global across the `core`, `metal`,
  `mgpu`, `model`, `moe`, `quant`, `server`, `spec` and `ui` areas
  (`patch_series llama` in the script);
- configures with these exact flags:

```sh
cmake -B build-static \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_METAL=ON \
  -DGGML_METAL_EMBED_LIBRARY=ON \
  -DTOSH_ENABLE_DYNAMIC_MOE=ON \
  -DGGML_NATIVE=OFF \
  -DCMAKE_OSX_ARCHITECTURES=@ARCH@ \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=@DEPLOYMENT_TARGET@ \
  -DCMAKE_CXX_FLAGS="-DTOSH_VERSION=@TOSH_VERSION@" \
  -DLLAMA_OPENSSL=OFF \
  -DGGML_SSE42=ON -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON \
  -DGGML_F16C=ON -DGGML_BMI2=ON -DGGML_AVX_VNNI=OFF -DGGML_AVX512=OFF
```

  (`TOSH_NO_AVX2=1` swaps the last six for the SSE4.2 baseline:
  `-DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_FMA=OFF -DGGML_F16C=OFF -DGGML_BMI2=OFF`.)
- builds:
  `cmake --build build-static --config Release -j "$(sysctl -n hw.ncpu)" -t llama-server llama-bench llama-perplexity test-backend-ops`;
- precompiles the Metal shaders, one `.metallib` per kernel source, into
  `build-static/bin/kernels/` when the Metal toolchain is available. Without it
  the shaders stay embedded and compile at first load.

## 4. Expected output

Run the commands from the archive root (the script returns there):

```
vendor/llama.cpp/build-static/bin/llama-server                  the shipped binary
vendor/llama.cpp/build-static/bin/kernels/*.metallib            optional precompiled kernels
```

`build.sh` copies `llama-server` and `kernels/` next to each other, copies
`LICENSE` as `LICENSE-GPL-3.0.txt`, and writes `engine.json` and `SHA256SUMS`;
the app looks for `llama-server` and its `kernels/` directory side by side.

Check the build:

```sh
vendor/llama.cpp/build-static/bin/llama-server --version
```

`SOURCE-OFFER.txt` records the pins and where this archive is published.
EOF

sed \
  -e "s|@TOSHLLM_TAG@|$TOSHLLM_TAG|g" \
  -e "s|@TOSHLLM_COMMIT@|$TOSHLLM_COMMIT|g" \
  -e "s|@TOSHLLM_REPO@|$TOSHLLM_REPO|g" \
  -e "s|@LLAMA_COMMIT@|$LLAMA_COMMIT|g" \
  -e "s|@LLAMA_REPO@|$llama_repo|g" \
  -e "s|@ARCH@|${ARCH:-x86_64}|g" \
  -e "s|@DEPLOYMENT_TARGET@|14.0|g" \
  -e "s|@TOSH_VERSION@|$tosh_version|g" \
  "$tmp/BUILDING.md.in" > "$stage/BUILDING.md"

{
  echo "Corresponding source offer - Locus bundled llama-server engine"
  echo
  echo "This archive is the complete corresponding source Locus publishes for the"
  echo "patched llama-server binary that ships inside Locus.app on macOS x86_64:"
  echo "the ToshLLM patch series and its build script at the pinned commit, the"
  echo "licence and copyright notices, the pins, and BUILDING.md, which rebuilds"
  echo "the binary from llama.cpp and these patches."
  echo
  echo "Engine:           ToshLLM-patched llama.cpp server (llama-server)"
  echo "Engine version:   ToshLLM $TOSHLLM_TAG"
  echo "ToshLLM project:  $TOSHLLM_REPO (commit $TOSHLLM_COMMIT)"
  echo "Upstream project: $llama_repo (commit $LLAMA_COMMIT)"
  echo "Target:           ${ARCH:-x86_64}-apple-darwin"
  echo "Licence:          GPL-3.0-or-later for the ToshLLM patches and for the"
  echo "                  patched binary built from llama.cpp (MIT) plus them;"
  echo "                  see LICENSE and patches/COPYRIGHT in this archive."
  echo
  echo "Published with every Locus release that ships the engine, next to the"
  echo "installer:"
  echo "  https://github.com/Astral420/Locus/releases"
  echo "  asset: llama-engine-source-$TOSHLLM_TAG.tar.gz"
  echo
  echo "The patch text in this archive is complete: rebuilding needs this archive"
  echo "and upstream llama.cpp at the commit above, not the ToshLLM repository."
  if [ "$test_checkout" = 1 ]; then
    echo
    echo "Note: built from a local test checkout at $commit (LOCUS_SOURCE_OFFER_SRC);"
    echo "not a published offer."
  fi
} > "$stage/SOURCE-OFFER.txt"

# Fixed mtime everywhere, POSIX tar headers, sorted entry names, no gzip name or
# timestamp: the same inputs give the same bytes on every run.
TZ=UTC find "$stage" -mindepth 1 -exec touch -t 202001010000.00 {} +
list="$tmp/entries"
( cd "$stage" && find . -mindepth 1 -print | sed 's|^\./||' | LC_ALL=C sort ) > "$list"

case "$(tar --version 2>/dev/null | head -1 || true)" in
  *"GNU tar"*) tar_flavor=(--no-recursion --owner=0 --group=0 --numeric-owner) ;;
  *)           tar_flavor=(-n --uid 0 --gid 0) ;;
esac

tar -cf "$tmp/archive.tar" -C "$stage" --format=ustar "${tar_flavor[@]}" -T "$list"
mkdir -p "$out_dir"
archive="$out_dir/llama-engine-source-$TOSHLLM_TAG.tar.gz"
gzip -n -9 -c "$tmp/archive.tar" > "$archive"

echo "entries: $(wc -l < "$list" | tr -d ' ')"
echo "source offer: $archive"
echo "sha256: $(sha256_file "$archive")"

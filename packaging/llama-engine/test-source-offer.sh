#!/usr/bin/env bash
# L1 test for source-offer.sh: no GPU, no model, no network, no engine build.
# Builds the archive twice from a throwaway ToshLLM checkout and checks that the
# bytes match and that every entry needed to rebuild is present.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
engine_root="$root/packaging/llama-engine"
script="$engine_root/source-offer.sh"
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

[ -x "$script" ] || die "missing or not executable: $script"
[ -f "$pin" ] || die "missing $pin"
. "$pin"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/locus-source-offer-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

# A ToshLLM checkout shaped like the real one for the files the offer needs.
fixture="$tmp/toshllm"
mkdir -p "$fixture/patches/llama/metal" "$fixture/patches/whisper" "$fixture/scripts"
printf 'GPL-3.0-or-later test fixture\n' > "$fixture/LICENSE"
printf 'Copyright (C) 2026 test fixture\nSPDX-License-Identifier: GPL-3.0-or-later\n' > "$fixture/patches/COPYRIGHT"
mkdir -p "$fixture/patches/llama/core"
printf 'fixture patch 0001\n' > "$fixture/patches/llama/metal/0001-fixture-metal.patch"
printf 'fixture patch 0002\n' > "$fixture/patches/llama/core/0002-fixture-core.patch"
printf 'fixture patch 0009\n' > "$fixture/patches/whisper/0009-fixture-whisper.patch"
printf '#!/bin/zsh\n# test fixture for scripts/build-engines.sh\n' > "$fixture/scripts/build-engines.sh"
printf '0.0.1-test\n' > "$fixture/VERSION"

git -C "$fixture" init -q
git -C "$fixture" add -A
git -C "$fixture" -c user.name="Locus test" -c user.email="test@example.invalid" \
  commit -qm "test fixture"

echo "1/3: first run"
LOCUS_SOURCE_OFFER_SRC="$fixture" "$script" > "$tmp/run1.log" 2>&1 \
  || { cat "$tmp/run1.log"; die "source-offer.sh failed on the first run"; }
archive="$out_dir/llama-engine-source-$TOSHLLM_TAG.tar.gz"
[ -f "$archive" ] || die "no archive at $archive"
grep -q "warning: LOCUS_SOURCE_OFFER_SRC is set" "$tmp/run1.log" \
  || die "the test-checkout warning is missing from the output"
first="$(sha256_file "$archive")"

echo "2/3: second run"
LOCUS_SOURCE_OFFER_SRC="$fixture" "$script" > "$tmp/run2.log" 2>&1 \
  || { cat "$tmp/run2.log"; die "source-offer.sh failed on the second run"; }
second="$(sha256_file "$archive")"

[ "$first" = "$second" ] || die "two runs produced different archives ($first != $second)"
echo "ok: two runs, one sha256 ($first)"

echo "3/3: archive contents"
tar tzf "$archive" > "$tmp/entries"
cat "$tmp/entries"

has_entry() { grep -qx "$1" "$tmp/entries"; }
has_prefix() { grep -q "^$1" "$tmp/entries"; }

has_prefix patches/ || die "archive is missing patches/"
has_entry "LICENSE" || die "archive is missing LICENSE"
if ! has_entry "patches/COPYRIGHT" && ! has_entry "COPYRIGHT"; then
  die "archive is missing patches/COPYRIGHT"
fi
has_entry "BUILDING.md" || die "archive is missing BUILDING.md"
has_entry "SOURCE-OFFER.txt" || die "archive is missing SOURCE-OFFER.txt"
has_entry "pin.env" || die "archive is missing pin.env"
has_entry "scripts/build-engines.sh" || die "archive is missing scripts/build-engines.sh"
grep -q "/build-engines.sh$" "$tmp/entries" || die "archive is missing build-engines.sh"

# The offer has to name what a rebuild needs, or it is not an offer.
tar xzf "$archive" -C "$tmp" BUILDING.md SOURCE-OFFER.txt
grep -q "$LLAMA_COMMIT" "$tmp/BUILDING.md" || die "BUILDING.md does not name the pinned llama.cpp commit"
grep -q "$LLAMA_COMMIT" "$tmp/SOURCE-OFFER.txt" || die "SOURCE-OFFER.txt does not name the pinned llama.cpp commit"
grep -q "$TOSHLLM_COMMIT" "$tmp/SOURCE-OFFER.txt" || die "SOURCE-OFFER.txt does not name the ToshLLM commit"
grep -q "GPL-3.0-or-later" "$tmp/SOURCE-OFFER.txt" || die "SOURCE-OFFER.txt does not name the licence"
grep -q "ggml-org/llama.cpp" "$tmp/SOURCE-OFFER.txt" || die "SOURCE-OFFER.txt does not name the upstream URL"

# Directory entries must exist too: BUILDING.md asks the reader to unpack it.
has_entry "patches/" || die "archive is missing the patches/ directory entry"
has_entry "scripts/" || die "archive is missing the scripts/ directory entry"

echo "ok: required entries and offer facts present"

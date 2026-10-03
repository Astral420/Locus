#!/usr/bin/env bash
# Downloads the pinned, checksum-verified static FFmpeg for one target into
# src-tauri/ffmpeg/ so the Tauri bundle ships it (bundle.resources: "ffmpeg").
#
#   packaging/fetch-ffmpeg.sh <target-triple>
#
# Env overrides (used by tests): MANIFEST, DEST.
set -euo pipefail

target="${1:?usage: fetch-ffmpeg.sh <target-triple>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="${MANIFEST:-$root/packaging/ffmpeg/manifest.txt}"
dest="${DEST:-$root/src-tauri/ffmpeg}"

line="$(grep -E "^${target}[[:space:]]" "$manifest" || true)"
[ -n "$line" ] || { echo "error: no FFmpeg entry for target '$target' in $manifest" >&2; exit 1; }
url="$(echo "$line" | awk '{print $2}')"
want="$(echo "$line" | awk '{print $3}')"
[ "$want" != "TODO" ] || { echo "error: sha256 for $target is still TODO. Run the build-ffmpeg workflow and paste its manifest lines." >&2; exit 1; }

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
echo "Downloading FFmpeg for $target"
curl --fail --location --silent --show-error --retry 3 -o "$tmp/ffmpeg.tar.gz" "$url"

got="$(sha256 "$tmp/ffmpeg.tar.gz")"
if [ "$got" != "$want" ]; then
  echo "error: sha256 mismatch for $target" >&2
  echo "  expected $want" >&2
  echo "  got      $got" >&2
  exit 1
fi

mkdir -p "$dest"
tar -xzf "$tmp/ffmpeg.tar.gz" -C "$tmp"
bin="ffmpeg"; case "$target" in *windows*) bin="ffmpeg.exe";; esac
[ -f "$tmp/$bin" ] || { echo "error: archive does not contain $bin at its root" >&2; exit 1; }
cp "$tmp/$bin" "$dest/$bin"
chmod +x "$dest/$bin"
# Ship the build record next to the binary (GPL source-offer / notices).
[ -f "$tmp/BUILDINFO.txt" ] && cp "$tmp/BUILDINFO.txt" "$dest/BUILDINFO.txt"
echo "Installed $dest/$bin"

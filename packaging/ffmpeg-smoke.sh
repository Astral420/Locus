#!/usr/bin/env bash
# Verifies an FFmpeg binary has everything Locus needs and is self-contained.
#   packaging/ffmpeg-smoke.sh <path-to-ffmpeg>
set -euo pipefail
ff="${1:?usage: ffmpeg-smoke.sh <ffmpeg>}"
fail() { echo "FFmpeg smoke test FAILED: $*" >&2; exit 1; }

"$ff" -hide_banner -version >/dev/null || fail "binary does not run"
enc="$("$ff" -hide_banner -encoders 2>/dev/null)"
echo "$enc" | grep -q "libx264" || fail "libx264 encoder missing"
echo "$enc" | grep -qE "^ A.* aac " || fail "aac encoder missing"
mux="$("$ff" -hide_banner -muxers 2>/dev/null)"; echo "$mux" | grep -qE " mp4 " || fail "mp4 muxer missing"
dem="$("$ff" -hide_banner -demuxers 2>/dev/null)"; echo "$dem" | grep -qE " concat " || fail "concat demuxer missing"
ind="$("$ff" -hide_banner -devices 2>/dev/null || true)"
case "$(uname -s)" in
  Darwin) echo "$ind" | grep -q avfoundation || fail "avfoundation input missing" ;;
  MINGW*|MSYS*|CYGWIN*) echo "$ind" | grep -q gdigrab || fail "gdigrab input missing" ;;
esac

# Real encode + fragmented segment + concat/mux, exactly like src/capture/screen.rs.
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
"$ff" -hide_banner -loglevel error -f lavfi -i "testsrc=size=321x241:rate=30:duration=1" \
  -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p" -c:v libx264 -preset veryfast \
  -movflags frag_keyframe+empty_moov+default_base_moof -y "$tmp/seg.mp4" || fail "libx264 segment encode"
"$ff" -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=1" -ar 48000 -ac 1 -y "$tmp/a.wav" || fail "make audio"
echo "file '$tmp/seg.mp4'" > "$tmp/list.txt"
"$ff" -hide_banner -loglevel error -f concat -safe 0 -i "$tmp/list.txt" -i "$tmp/a.wav" \
  -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 160k -movflags +faststart -f mp4 -y "$tmp/out.mp4" || fail "mux"
"$ff" -v error -i "$tmp/out.mp4" -f null - || fail "output does not decode"

# Must not depend on libraries that only exist on the build machine.
case "$(uname -s)" in
  Darwin) if otool -L "$ff" | tail -n +2 | grep -vE "^\s*(/usr/lib/|/System/Library/)"; then fail "links non-system dylibs (not relocatable)"; fi ;;
  Linux)  if ldd "$ff" 2>&1 | grep -vE "not a dynamic|linux-vdso|libc\.so|libm\.so|libpthread|libdl|librt|ld-linux|libgcc_s|libstdc"; then fail "links unexpected shared libs"; fi ;;
esac
echo "FFmpeg smoke test OK: $("$ff" -version | head -1)"

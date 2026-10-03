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

# Real encode + fragmented segment + normalise + concat/mux, using exactly the
# options src/capture/screen.rs uses in production (filters, -r, zerolatency,
# -progress, setts). A build missing any of them fails here, not on a user's Mac.
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
"$ff" -hide_banner -loglevel error -nostats -progress pipe:1 -stats_period 0.1 \
  -f lavfi -i "testsrc=size=321x241:rate=30:duration=1" -an \
  -vf "setpts=PTS-STARTPTS,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p" \
  -c:v libx264 -preset veryfast -tune zerolatency -crf 26 -r 30 -g 30 \
  -movflags frag_keyframe+empty_moov+default_base_moof -y "$tmp/seg.mp4" > "$tmp/progress.txt" \
  || fail "libx264 segment encode (needs filters setpts,scale,format,fps)"
grep -q '^frame=' "$tmp/progress.txt" || fail "-progress output missing"
"$ff" -hide_banner -loglevel error -i "$tmp/seg.mp4" -map 0:v:0 -c copy -bsf:v "setts=ts=TS-STARTDTS" -an -f mp4 -y "$tmp/norm.mp4" \
  || fail "setts bitstream filter missing"
"$ff" -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=1" -ar 48000 -ac 1 -y "$tmp/a.wav" || fail "make audio"
echo "file '$tmp/norm.mp4'" > "$tmp/list.txt"
"$ff" -hide_banner -loglevel error -f concat -safe 0 -i "$tmp/list.txt" -ss 0.2 -i "$tmp/a.wav" \
  -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 160k -movflags +faststart -f mp4 -y "$tmp/out.mp4" || fail "mux"
"$ff" -v error -i "$tmp/out.mp4" -f null - || fail "output does not decode"

# Must not depend on libraries that only exist on the build machine.
# (SMOKE_SKIP_LINK_CHECK=1 is for testing a dynamically linked dev FFmpeg only.)
case "${SMOKE_SKIP_LINK_CHECK:+skip}$(uname -s)" in
  skip*) ;;
  Darwin) if otool -L "$ff" | tail -n +2 | grep -vE "^\s*(/usr/lib/|/System/Library/)"; then fail "links non-system dylibs (not relocatable)"; fi ;;
  Linux)  if ldd "$ff" 2>&1 | grep -vE "not a dynamic|linux-vdso|libc\.so|libm\.so|libmvec|libpthread|libdl|librt|ld-linux|libgcc_s|libstdc"; then fail "links unexpected shared libs"; fi ;;
esac
echo "FFmpeg smoke test OK: $("$ff" -version | head -1)"

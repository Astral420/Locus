#!/usr/bin/env bash
# Verifies an FFmpeg binary has everything Locus needs to ENCODE and MUX, and is
# self-contained.  FFmpeg never captures the screen in the native design
# (NATIVE_CAPTURE_PLAN §1), so no capture device is required or checked.
#   packaging/ffmpeg-smoke.sh <path-to-ffmpeg>
#
# Every command below uses the options the production code uses, so a component
# missing from the build fails here, in CI, and not on a user's machine:
#   - raw frames on a pipe -> libx264 fragmented-MP4 segment
#       src-tauri/src/capture/video_encoder.rs  (SegmentEncoder::start)
#   - segment re-time (setts)                    capture/screen.rs  (normalize_segment)
#   - concat + WAV inputs -> MP4 with up to three AAC streams, faststart
#       capture/container.rs                      (mux_container)
#   - full decode of the result (-f null)         capture/screen.rs  (verification)
# If one of those command lines changes, change it here too.
set -euo pipefail
ff="${1:?usage: ffmpeg-smoke.sh <ffmpeg>}"
fail() { echo "FFmpeg smoke test FAILED: $*" >&2; exit 1; }

"$ff" -hide_banner -version >/dev/null || fail "binary does not run"

# Component inventory first: a clear message beats a cryptic encode error.
enc="$("$ff" -hide_banner -encoders 2>/dev/null)"
echo "$enc" | grep -q "libx264" || fail "libx264 encoder missing"
echo "$enc" | grep -qE "^ A.* aac " || fail "aac encoder missing"
mux="$("$ff" -hide_banner -muxers 2>/dev/null)"
for m in mp4 null; do echo "$mux" | grep -qE " $m " || fail "$m muxer missing"; done
dem="$("$ff" -hide_banner -demuxers 2>/dev/null)"
for d in concat mov wav rawvideo; do echo "$dem" | grep -qE " $d[ ,]" || fail "$d demuxer missing"; done
dec="$("$ff" -hide_banner -decoders 2>/dev/null)"
for d in h264 aac rawvideo pcm_s16le; do echo "$dec" | grep -qE " $d " || fail "$d decoder missing"; done

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

# --- fixtures, built without FFmpeg so the binary under test stays minimal ----
# Mono 48 kHz 16-bit PCM WAV, the format capture/audio.rs writes (hound).
byte() { printf "\\x$(printf %02x "$(( $1 & 255 ))")"; }
le16() { byte "$1"; byte $(( $1 >> 8 )); }
le32() { byte "$1"; byte $(( $1 >> 8 )); byte $(( $1 >> 16 )); byte $(( $1 >> 24 )); }
make_wav() { # <file> <seconds>
  local bytes=$(( 48000 * 2 * $2 ))
  { printf 'RIFF'; le32 $(( 36 + bytes )); printf 'WAVEfmt '; le32 16; le16 1; le16 1
    le32 48000; le32 96000; le16 2; le16 16; printf 'data'; le32 "$bytes"
    head -c "$bytes" /dev/urandom; } > "$1"
}
make_wav "$tmp/mixed.wav" 2; make_wav "$tmp/system.wav" 2; make_wav "$tmp/mic.wav" 2

# --- 1. raw frames on stdin -> fragmented H.264 segment (both pixel formats) ---
# NV12 comes from ScreenCaptureKit, BGRA from Windows Graphics Capture and
# PipeWire. 30 frames of 322x242 (even, deliberately not a multiple of 16).
W=322; H=242; N=30
encode_segment() { # <pix_fmt> <frame bytes> <out>
  head -c $(( $2 * N )) /dev/urandom | "$ff" -hide_banner -loglevel error -nostats \
    -f rawvideo -pix_fmt "$1" -s "${W}x${H}" -framerate 30 -i pipe:0 -an \
    -vf format=yuv420p -c:v libx264 -preset veryfast -tune zerolatency -crf 26 -r 30 -g 30 \
    -movflags frag_keyframe+empty_moov+default_base_moof -y "$3" \
    || fail "segment encode from rawvideo $1 (needs rawvideo demuxer+decoder, pipe protocol, filters format,scale,fps, libx264, mp4 muxer)"
  [ -s "$3" ] || fail "segment encode from $1 wrote nothing"
}
encode_segment nv12 $(( W * H * 3 / 2 )) "$tmp/seg-nv12.mp4"
encode_segment bgra $(( W * H * 4 ))     "$tmp/seg-bgra.mp4"

# --- 2. lossless re-time of each segment (normalize_segment) ------------------
for s in nv12 bgra; do
  "$ff" -hide_banner -loglevel error -i "$tmp/seg-$s.mp4" -map 0:v:0 -c copy \
    -bsf:v "setts=ts=TS-STARTDTS" -an -f mp4 -y "$tmp/norm-$s.mp4" \
    || fail "segment re-time (needs the setts bitstream filter and the mov demuxer)"
done

# --- 3. final container: video + three audio streams (mux_container) ----------
# A pause produces several segments, so concat two; audio is delayed
# (-itsoffset) or its head trimmed (-ss), exactly as the code does.
{ echo "file '$tmp/norm-nv12.mp4'"; echo "file '$tmp/norm-bgra.mp4'"; } > "$tmp/list.txt"
audio_maps() {
  echo -n "-map 1:a:0 -b:a:0 160k -metadata:s:a:0 handler_name=mixed -disposition:a:0 default "
  echo -n "-map 2:a:0 -b:a:1 96k -metadata:s:a:1 handler_name=system -disposition:a:1 0 "
  echo -n "-map 3:a:0 -b:a:2 96k -metadata:s:a:2 handler_name=microphone -disposition:a:2 0"
}
# shellcheck disable=SC2046
"$ff" -hide_banner -loglevel error -f concat -safe 0 -i "$tmp/list.txt" \
  -itsoffset 0.250 -i "$tmp/mixed.wav" -itsoffset 0.250 -i "$tmp/system.wav" \
  -itsoffset 0.250 -i "$tmp/mic.wav" \
  -map 0:v:0 -c:v copy $(audio_maps) -c:a aac -movflags +faststart -f mp4 -y "$tmp/out-delay.mp4" \
  || fail "mux with delayed audio (needs concat+wav demuxers, aac encoder, aresample/aformat filters)"
# shellcheck disable=SC2046
"$ff" -hide_banner -loglevel error -f concat -safe 0 -i "$tmp/list.txt" \
  -ss 0.500 -i "$tmp/mixed.wav" -ss 0.500 -i "$tmp/system.wav" -ss 0.500 -i "$tmp/mic.wav" \
  -map 0:v:0 -c:v copy $(audio_maps) -c:a aac -movflags +faststart -f mp4 -y "$tmp/out-trim.mp4" \
  || fail "mux with trimmed audio"
# Audio-only session: no video stream at all, no dummy video (FR1.11).
"$ff" -hide_banner -loglevel error -i "$tmp/mixed.wav" -i "$tmp/mic.wav" \
  -map 0:a:0 -b:a:0 160k -metadata:s:a:0 handler_name=mixed -disposition:a:0 default \
  -map 1:a:0 -b:a:1 96k -metadata:s:a:1 handler_name=microphone -disposition:a:1 0 \
  -c:a aac -vn -movflags +faststart -f mp4 -y "$tmp/out-audio.mp4" \
  || fail "audio-only mux"

# --- 4. the result must fully decode and carry the expected streams -----------
streams() { "$ff" -hide_banner -i "$1" 2>&1 || true; }  # -i alone exits non-zero by design
for f in out-delay out-trim; do
  "$ff" -v error -i "$tmp/$f.mp4" -f null - || fail "$f.mp4 does not decode (needs h264 + aac decoders, null muxer)"
  info="$(streams "$tmp/$f.mp4")"
  [ "$(echo "$info" | grep -c 'Video: h264')" = 1 ] || fail "$f.mp4: expected exactly one H.264 video stream"
  [ "$(echo "$info" | grep -c 'Audio: aac')" = 3 ] || fail "$f.mp4: expected three AAC audio streams"
done
"$ff" -v error -i "$tmp/out-audio.mp4" -f null - || fail "audio-only mp4 does not decode"
info="$(streams "$tmp/out-audio.mp4")"
echo "$info" | grep -q 'Video:' && fail "audio-only mp4 must have no video stream"
[ "$(echo "$info" | grep -c 'Audio: aac')" = 2 ] || fail "audio-only mp4: expected two AAC audio streams"

# Must not depend on libraries that only exist on the build machine.
# (SMOKE_SKIP_LINK_CHECK=1 is for testing a dynamically linked dev FFmpeg only.)
case "${SMOKE_SKIP_LINK_CHECK:+skip}$(uname -s)" in
  skip*) ;;
  Darwin) if otool -L "$ff" | tail -n +2 | grep -vE "^\s*(/usr/lib/|/System/Library/)"; then fail "links non-system dylibs (not relocatable)"; fi ;;
  Linux)  if ldd "$ff" 2>&1 | grep -vE "not a dynamic|linux-vdso|libc\.so|libm\.so|libmvec|libpthread|libdl|librt|ld-linux|libgcc_s|libstdc"; then fail "links unexpected shared libs"; fi ;;
esac
echo "FFmpeg smoke test OK: $("$ff" -version | head -1)"

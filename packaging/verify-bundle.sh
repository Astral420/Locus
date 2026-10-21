#!/usr/bin/env bash
# Fails unless the built installer/bundle actually contains the bundled FFmpeg.
# On x86_64 macOS it also requires the bundled llama-server engine.
#   packaging/verify-bundle.sh <macos|windows|linux>
#
# VERIFY_SKIP_SIGNATURES=1 skips only the codesign checks, for local unsigned
# builds. It is refused when CI is set; release verification must be signed.
set -euo pipefail
os="${1:?usage: verify-bundle.sh <macos|windows|linux>}"
bundle="src-tauri/target"
fail() { echo "Bundle verification FAILED: $*" >&2; exit 1; }
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

case "$os" in
  macos)
    app="$(find "$bundle" -type d -name 'Locus.app' -path '*bundle/macos*' | head -1)"
    [ -n "$app" ] || fail "Locus.app not found"
    ff="$app/Contents/Resources/ffmpeg/ffmpeg"
    [ -x "$ff" ] || fail "ffmpeg missing or not executable in $app/Contents/Resources/ffmpeg"
    skip_signatures=0
    if [ "${VERIFY_SKIP_SIGNATURES:-}" = "1" ]; then
      [ -z "${CI:-}" ] || fail "VERIFY_SKIP_SIGNATURES is not allowed in CI"
      skip_signatures=1
      echo "note: VERIFY_SKIP_SIGNATURES=1; skipping codesign checks"
    fi
    if [ "$skip_signatures" = 0 ]; then
      codesign --verify --strict --verbose=2 "$ff" || fail "bundled ffmpeg is not validly code-signed"
      codesign --verify --deep --strict "$app" || fail "app signature invalid"
    fi
    packaging/ffmpeg-smoke.sh "$ff"

    exe="$app/Contents/MacOS/Locus"
    [ -x "$exe" ] || fail "app executable not found at $exe"
    archs="$(lipo -archs "$exe")" || fail "cannot read architectures from $exe"
    case " $archs " in
      *" x86_64 "*)
        eng="$app/Contents/Resources/llama-engine"
        llama="$eng/llama-server"
        [ -x "$llama" ] || fail "llama-server missing or not executable in $eng"
        [ -f "$eng/engine.json" ] || fail "engine.json missing from $eng"
        [ -d "$eng/kernels" ] || fail "kernels/ missing from $eng"
        if [ "$skip_signatures" = 0 ]; then
          codesign --verify --strict --verbose=2 "$llama" || fail "bundled llama-server is not validly code-signed"
        fi
        "$llama" --version >/dev/null 2>&1 || fail "bundled llama-server --version failed"
        command -v python3 >/dev/null 2>&1 || fail "python3 is required to read engine.json"
        eng_arch="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["arch"])' "$eng/engine.json")" \
          || fail "cannot parse $eng/engine.json"
        [ "$eng_arch" = "x86_64" ] || fail "engine.json arch is '$eng_arch', expected 'x86_64'"
        ;;
      *)
        echo "note: $archs app; no bundled llama-engine checks"
        ;;
    esac
    ;;
  linux)
    img="$(find "$bundle" -name '*.AppImage' | head -1)"
    [ -n "$img" ] || fail "AppImage not found"
    chmod +x "$img"; (cd "$tmp" && "$OLDPWD/$img" --appimage-extract >/dev/null)
    ff="$(find "$tmp/squashfs-root" -path '*ffmpeg/ffmpeg' -type f | head -1)"
    [ -n "$ff" ] || fail "ffmpeg not inside the AppImage"
    # Locus looks for ../lib/Locus/ffmpeg/ relative to usr/bin.
    case "$ff" in */usr/lib/Locus/ffmpeg/ffmpeg) ;; *) fail "unexpected location $ff (expected usr/lib/Locus/ffmpeg/)";; esac
    packaging/ffmpeg-smoke.sh "$ff"
    ;;
  windows)
    exe="$(find "$bundle" -path '*bundle/nsis*' -name '*-setup.exe' | head -1)"
    [ -n "$exe" ] || fail "NSIS installer not found"
    7z x -y -o"$tmp" "$exe" >/dev/null
    ff="$(find "$tmp" -iname 'ffmpeg.exe' | head -1)"
    [ -n "$ff" ] || fail "ffmpeg.exe not inside the installer"
    case "$ff" in */ffmpeg/ffmpeg.exe) ;; *) fail "unexpected location $ff (expected ffmpeg/ffmpeg.exe next to Locus.exe)";; esac
    packaging/ffmpeg-smoke.sh "$ff"
    ;;
  *) fail "unknown os $os" ;;
esac
echo "Bundle verification OK ($os)"

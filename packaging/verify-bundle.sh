#!/usr/bin/env bash
# Fails unless the built installer/bundle actually contains the bundled FFmpeg.
#   packaging/verify-bundle.sh <macos|windows|linux>
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
    codesign --verify --strict --verbose=2 "$ff" || fail "bundled ffmpeg is not validly code-signed"
    codesign --verify --deep --strict "$app" || fail "app signature invalid"
    packaging/ffmpeg-smoke.sh "$ff"
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

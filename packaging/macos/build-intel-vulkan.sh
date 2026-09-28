#!/usr/bin/env bash
set -euo pipefail

# M11 release build guard. The script refuses to package floating or missing
# artifacts; the release operator must provide pinned, reviewed binaries.
source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
artifact_root="${LOCUS_VULKAN_ARTIFACT_ROOT:-${source_root}/packaging/macos/vulkan-artifacts}"
for artifact in whisper-vulkan llama-server-vulkan MoltenVK.dylib; do
  if [[ ! -f "${artifact_root}/${artifact}" ]]; then
    echo "missing qualified artifact: ${artifact_root}/${artifact}" >&2
    exit 1
  fi
done

manifest="${artifact_root}/SHA256SUMS"
(cd "${artifact_root}" && shasum -a 256 whisper-vulkan llama-server-vulkan MoltenVK.dylib > "${manifest}")
echo "Pinned artifact checksums written to ${manifest}"

# The ordinary Tauri config keeps Apple Silicon on Metal. Use the explicit
# Intel target only after the checksum and hardware evidence are attached.
pnpm --dir "${source_root}" tauri build --target x86_64-apple-darwin --bundles dmg

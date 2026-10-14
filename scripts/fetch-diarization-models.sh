#!/usr/bin/env bash
# Downloads the speaker-detection models into models/diarization/ and verifies their checksums.
#   scripts/fetch-diarization-models.sh [destination-folder]
#
# segmentation.onnx  pyannote segmentation 3.0 exported by sherpa-onnx (MIT, CNRS).
# embedding.onnx     NVIDIA NeMo TitaNet-small speaker embedding (check its licence terms before shipping).
set -euo pipefail
cd "$(dirname "$0")/.."
DEST="${1:-models/diarization}"
mkdir -p "$DEST"

sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
BASE="https://github.com/k2-fsa/sherpa-onnx/releases/download"

SEGMENTATION_TAR_SHA="24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488"
SEGMENTATION_SHA="220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079"
EMBEDDING_SHA="ad4a1802485d8b34c722d2a9d04249662f2ece5d28a7a039063ca22f515a789e"

have() { [ -f "$1" ] && [ "$(sha256 "$1")" = "$2" ]; }

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT

if have "$DEST/segmentation.onnx" "$SEGMENTATION_SHA"; then
  echo "ok   segmentation.onnx"
else
  curl -fL --retry 3 -o "$work/seg.tar.bz2" "$BASE/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2"
  [ "$(sha256 "$work/seg.tar.bz2")" = "$SEGMENTATION_TAR_SHA" ] || { echo "segmentation download failed its checksum" >&2; exit 1; }
  tar -xjf "$work/seg.tar.bz2" -C "$work"
  [ "$(sha256 "$work/sherpa-onnx-pyannote-segmentation-3-0/model.onnx")" = "$SEGMENTATION_SHA" ] || { echo "segmentation model failed its checksum" >&2; exit 1; }
  cp "$work/sherpa-onnx-pyannote-segmentation-3-0/model.onnx" "$DEST/segmentation.onnx"
  cp "$work/sherpa-onnx-pyannote-segmentation-3-0/LICENSE" "$DEST/segmentation.LICENSE"
  echo "got  segmentation.onnx"
fi

if have "$DEST/embedding.onnx" "$EMBEDDING_SHA"; then
  echo "ok   embedding.onnx"
else
  curl -fL --retry 3 -o "$work/embedding.onnx" "$BASE/speaker-recongition-models/nemo_en_titanet_small.onnx"
  [ "$(sha256 "$work/embedding.onnx")" = "$EMBEDDING_SHA" ] || { echo "embedding model failed its checksum" >&2; exit 1; }
  cp "$work/embedding.onnx" "$DEST/embedding.onnx"
  echo "got  embedding.onnx"
fi
echo "Speaker-detection models are in $DEST"

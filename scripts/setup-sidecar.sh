#!/usr/bin/env bash
# One-time setup for running Locus from source (`pnpm tauri dev`): creates .venv with everything the
# sidecar needs (slide detection, text recognition, speaker detection, search) and fetches the models.
# Locus finds .venv by itself; there is nothing to activate.
#   scripts/setup-sidecar.sh [--skip-models]
set -euo pipefail
cd "$(dirname "$0")/.."
PYTHON="${PYTHON:-python3}"

"$PYTHON" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' \
  || { echo "Python 3.10 or newer is required (set PYTHON=/path/to/python)." >&2; exit 1; }

"$PYTHON" -m venv .venv
.venv/bin/python -m pip install --quiet --upgrade pip
.venv/bin/python -m pip install -r sidecar/requirements.txt
# RapidOCR bundles its models; --no-deps because its other requirements are already in requirements.txt.
.venv/bin/python -m pip install --no-deps "rapidocr-onnxruntime==1.4.4"

case " $* " in
  *" --skip-models "*) echo "Skipping model download." ;;
  *) scripts/fetch-diarization-models.sh ;;
esac
echo "Sidecar is ready. Start the app as usual: pnpm tauri dev"

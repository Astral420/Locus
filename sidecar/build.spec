"""PyInstaller recipe; model and native assets are copied only from local paths.

Everything the default features need ends up inside the bundle, so the packaged app needs no
Python and no PyTorch on the user's machine:
  * RapidOCR (slide text) - its models are package data and are collected with it,
  * sherpa-onnx + onnxruntime (speaker detection) - native libraries are collected,
  * models/diarization/*.onnx (fetched by scripts/fetch-diarization-models.sh) -> <bundle>/models/diarization,
    which sidecar/src/resources.py searches.
An optional pyannote model is still bundled when provided.
"""
import os
from pathlib import Path
from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs, collect_submodules

ROOT = Path(SPECPATH)  # the sidecar/ folder
REPO = ROOT.parent
MODEL_ROOT = Path(os.environ.get("LOCUS_PYANNOTE_MODEL", ROOT / "models" / "pyannote" / "speaker-diarization-community-1"))
datas = [(str(MODEL_ROOT), "models/pyannote/speaker-diarization-community-1")] if MODEL_ROOT.exists() else []
hiddenimports = collect_submodules("sidecar.src")
binaries = []

DIARIZATION_MODELS = REPO / "models" / "diarization"
if (DIARIZATION_MODELS / "segmentation.onnx").exists() and (DIARIZATION_MODELS / "embedding.onnx").exists():
    datas.append((str(DIARIZATION_MODELS), "models/diarization"))

for package in ("rapidocr_onnxruntime", "sherpa_onnx", "onnxruntime", "chromadb"):
    try:
        hiddenimports += collect_submodules(package)
        datas += collect_data_files(package)
        binaries += collect_dynamic_libs(package)
    except Exception:
        pass  # the package is not installed in this build environment

a = Analysis(
    [str(ROOT / "src" / "main.py")],
    pathex=[str(ROOT.parent)],
    datas=datas,
    binaries=binaries,
    hiddenimports=hiddenimports,
    excludes=["tkinter"],
)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, a.binaries, a.datas, name="locus-sidecar", console=True)

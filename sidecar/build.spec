"""PyInstaller recipe; model and native assets are copied only from local paths.

Everything the default features need ends up inside the bundle, so the packaged app needs no
Python, no Tesseract and no PyTorch on the user's machine:
  * RapidOCR (slide text) - its models are package data and are collected with it,
  * sherpa-onnx + onnxruntime (speaker detection) - native libraries are collected,
  * models/diarization/*.onnx (fetched by scripts/fetch-diarization-models.sh) -> <bundle>/models/diarization,
    which sidecar/src/resources.py searches.
Optional alternatives (Tesseract via LOCUS_TESSERACT, a pyannote model) are still bundled when provided.
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

binary_path = os.environ.get("LOCUS_TESSERACT")
if binary_path and Path(binary_path).exists():
    binaries.append((binary_path, "."))
tessdata = os.environ.get("TESSDATA_PREFIX")
if tessdata and Path(tessdata).exists():
    datas.append((tessdata, "tessdata"))

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

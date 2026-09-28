"""PyInstaller recipe; model and native assets are copied only from local paths."""
import os
from pathlib import Path
from PyInstaller.utils.hooks import collect_submodules

ROOT = Path(__file__).parent
MODEL_ROOT = Path(os.environ.get("LOCUS_PYANNOTE_MODEL", ROOT / "models" / "pyannote" / "speaker-diarization-community-1"))
datas = [(str(MODEL_ROOT), "models/pyannote/speaker-diarization-community-1")] if MODEL_ROOT.exists() else []
hiddenimports = collect_submodules("sidecar.src")
binary_path = os.environ.get("LOCUS_TESSERACT")
binaries = [(binary_path, ".")] if binary_path and Path(binary_path).exists() else []
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

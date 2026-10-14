"""Finding files that ship with Locus: models and helper programs.

The packaged sidecar used to rely on environment variables that nothing set, so even a
correctly built bundle could not find the models or Tesseract it contained. Lookup order:

1. ``LOCUS_MODEL_DIRS`` - set by the host app (app-data models folder, then the bundled one),
2. the PyInstaller bundle (``sys._MEIPASS/models``) when frozen,
3. the repository ``models/`` folder when running from a source checkout.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

ENV_MODEL_DIRS = "LOCUS_MODEL_DIRS"


def model_dirs() -> list[Path]:
    candidates = [Path(raw) for raw in os.environ.get(ENV_MODEL_DIRS, "").split(os.pathsep) if raw]
    frozen = getattr(sys, "_MEIPASS", None)
    if frozen:
        candidates.append(Path(frozen) / "models")
    candidates.append(Path(__file__).resolve().parents[2] / "models")
    unique: list[Path] = []
    for candidate in candidates:
        if candidate not in unique:
            unique.append(candidate)
    return unique


def find_model(*relative: str) -> Path | None:
    for base in model_dirs():
        path = base.joinpath(*relative)
        if path.exists():
            return path
    return None


def bundled_executable(name: str) -> Path | None:
    """A helper program placed next to the frozen sidecar (e.g. Tesseract)."""
    frozen = getattr(sys, "_MEIPASS", None)
    if not frozen:
        return None
    for candidate in (Path(frozen) / name, Path(frozen) / f"{name}.exe", Path(frozen) / name / name):
        if candidate.is_file():
            return candidate
    return None

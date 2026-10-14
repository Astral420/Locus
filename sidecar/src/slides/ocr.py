"""Slide text recognition.

Engines, in order of preference:

1. RapidOCR - a pip wheel that contains its own models, so it needs no external program and can be
   bundled with the app as ordinary Python packages.
2. Tesseract (pytesseract + the ``tesseract`` program), for installs that already have it.

If neither is present the call raises ``CapabilityUnavailable``; the host then skips the step with an
honest message instead of reporting a crash.
"""

from __future__ import annotations

import os

from ..errors import CapabilityUnavailable
from ..resources import bundled_executable

MIN_CONFIDENCE = 0.5
_rapidocr_engine = None


def _rapidocr():
    """The shared RapidOCR engine, or None when it is not installed (creating it loads the models once)."""
    global _rapidocr_engine
    if _rapidocr_engine is None:
        try:
            from rapidocr_onnxruntime import RapidOCR  # type: ignore
        except ImportError:
            return None
        _rapidocr_engine = RapidOCR()
    return _rapidocr_engine


def _read_with_rapidocr(engine, image_path: str) -> str:
    result, _ = engine(image_path)
    # RapidOCR returns lines top-to-bottom as [box, text, confidence]; low-confidence lines are usually noise.
    return "\n".join(text.strip() for _box, text, confidence in (result or []) if float(confidence) >= MIN_CONFIDENCE and text.strip())


def _read_with_tesseract(image_path: str) -> str:
    try:
        import pytesseract  # type: ignore
        from PIL import Image  # type: ignore
    except ImportError as exc:
        raise CapabilityUnavailable("no text recognition engine is installed (run scripts/setup-sidecar.sh)") from exc
    command = os.environ.get("LOCUS_TESSERACT") or (str(path) if (path := bundled_executable("tesseract")) else None)
    if command:
        pytesseract.pytesseract.tesseract_cmd = command
    try:
        return pytesseract.image_to_string(Image.open(image_path)).strip()
    except pytesseract.TesseractNotFoundError as exc:
        raise CapabilityUnavailable(
            "no text recognition engine is installed (run scripts/setup-sidecar.sh, or install Tesseract)"
        ) from exc


def ocr_slide(image_path: str) -> str:
    engine = _rapidocr()
    if engine is not None:
        return _read_with_rapidocr(engine, image_path)
    return _read_with_tesseract(image_path)

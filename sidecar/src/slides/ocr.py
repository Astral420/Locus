"""Tesseract OCR adapter with explicit, non-crashing dependency errors."""

from __future__ import annotations

import os

from ..errors import CapabilityUnavailable


def ocr_slide(image_path: str) -> str:
    try:
        import pytesseract  # type: ignore
        from PIL import Image  # type: ignore
    except ImportError as exc:
        raise CapabilityUnavailable(
            "OCR needs the Python packages pytesseract and Pillow (pip install pytesseract Pillow)"
        ) from exc
    tesseract_cmd = os.environ.get("LOCUS_TESSERACT")
    if tesseract_cmd:
        pytesseract.pytesseract.tesseract_cmd = tesseract_cmd
    try:
        return pytesseract.image_to_string(Image.open(image_path)).strip()
    except pytesseract.TesseractNotFoundError as exc:
        raise CapabilityUnavailable(
            "OCR needs the Tesseract program (macOS: brew install tesseract, or set LOCUS_TESSERACT)"
        ) from exc

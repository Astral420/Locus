"""Tesseract OCR adapter with stderr-safe, explicit dependency errors."""

from __future__ import annotations

import os


def ocr_slide(image_path: str) -> str:
    try:
        import pytesseract  # type: ignore
        from PIL import Image  # type: ignore
    except ImportError as exc:
        raise RuntimeError("pytesseract and Pillow are required for OCR") from exc
    tesseract_cmd = os.environ.get("LOCUS_TESSERACT")
    if tesseract_cmd:
        pytesseract.pytesseract.tesseract_cmd = tesseract_cmd
    return pytesseract.image_to_string(Image.open(image_path)).strip()

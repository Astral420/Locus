"""Slide text recognition with RapidOCR.

RapidOCR is a pip package that carries its own detection and recognition models, so it needs no
external program and bundles with the sidecar as ordinary Python packages. There is deliberately no
second engine: a fallback that needs a native program (and per-language data) would be code that
the packaged app never runs.

If RapidOCR is not installed the call raises ``CapabilityUnavailable``; the host then skips the step
with an honest message instead of reporting a crash.
"""

from __future__ import annotations

from ..errors import CapabilityUnavailable

MIN_CONFIDENCE = 0.5
_engine = None


def _rapidocr():
    """The shared RapidOCR engine (creating it loads the models once)."""
    global _engine
    if _engine is None:
        try:
            from rapidocr_onnxruntime import RapidOCR  # type: ignore
        except ImportError as error:
            raise CapabilityUnavailable(
                "text recognition (RapidOCR) is not installed (run scripts/setup-sidecar.sh)"
            ) from error
        _engine = RapidOCR()
    return _engine


def ocr_slide(image_path: str) -> str:
    result, _ = _rapidocr()(image_path)
    # RapidOCR returns lines top-to-bottom as [box, text, confidence]; low-confidence lines are usually noise.
    return "\n".join(
        text.strip() for _box, text, confidence in (result or []) if float(confidence) >= MIN_CONFIDENCE and text.strip()
    )

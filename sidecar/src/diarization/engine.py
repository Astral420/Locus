"""Speaker diarization adapter: torch-free ONNX engine first, pyannote.audio if it is installed and configured."""

from __future__ import annotations

import os
from collections.abc import Callable

from ..resources import find_model
from .onnx_engine import DiarizationUnavailable, diarize_onnx
from .vad import vad

__all__ = ["DiarizationUnavailable", "diarize"]


def diarize(audio_path: str, single_person: bool = False, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, object]]:
    if single_person:
        regions = vad(audio_path)
        result = [{"start": region["start"], "end": region["end"], "speaker": "User", "source": "microphone"} for region in regions]
        if progress:
            progress(1.0)
        return result

    # Preferred: the torch-free ONNX engine, which can ship inside the app. pyannote.audio stays
    # supported for installs that already have it plus a local model.
    reasons: list[str] = []
    try:
        return diarize_onnx(audio_path, progress)
    except DiarizationUnavailable as error:
        reasons.append(str(error))
    try:
        return _diarize_pyannote(audio_path, progress)
    except DiarizationUnavailable as error:
        reasons.append(str(error))
    raise DiarizationUnavailable("; ".join(reasons))


def _diarize_pyannote(audio_path: str, progress: Callable[[float, str | None], None] | None) -> list[dict[str, object]]:
    try:
        from pyannote.audio import Pipeline  # type: ignore
    except ImportError as error:
        raise DiarizationUnavailable("pyannote.audio is not installed") from error

    bundled = find_model("pyannote", "speaker-diarization-community-1")
    model_path = os.environ.get("LOCUS_PYANNOTE_MODEL") or (str(bundled) if bundled else None)
    if not model_path:
        raise DiarizationUnavailable("no local pyannote model is configured (LOCUS_PYANNOTE_MODEL)")
    pipeline = Pipeline.from_pretrained(model_path)
    annotation = pipeline(audio_path)
    result = [
        {"start": float(segment.start), "end": float(segment.end), "speaker": str(label), "source": "mixed"}
        for segment, _, label in annotation.itertracks(yield_label=True)
    ]
    if progress:
        progress(1.0)
    return result

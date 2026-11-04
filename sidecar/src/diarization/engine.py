"""Speaker diarization adapter: torch-free ONNX engine first, pyannote.audio if it is installed and configured."""

from __future__ import annotations

import os
from collections.abc import Callable, Mapping
from typing import Any

from ..resources import find_model
from .onnx_engine import (
    DiarizationSettings,
    DiarizationUnavailable,
    audio_seconds,
    diarize_onnx,
    diarize_onnx_with_diagnostics,
)
from .postprocess import speaker_summary
from .vad import vad

__all__ = ["DiarizationSettings", "DiarizationUnavailable", "diarize", "diarize_with_diagnostics"]


def diarize(
    audio_path: str,
    single_person: bool = False,
    progress: Callable[[float, str | None], None] | None = None,
    settings: Mapping[str, Any] | None = None,
) -> list[dict[str, object]]:
    return diarize_with_diagnostics(audio_path, single_person, progress, settings)["segments"]


def diarize_with_diagnostics(
    audio_path: str,
    single_person: bool = False,
    progress: Callable[[float, str | None], None] | None = None,
    settings: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Speaker segments plus how they were produced.

    ``settings`` carries per-call tuning from the host (or the sweep script); the environment
    variables documented in :class:`DiarizationSettings` stay the default. Whatever the engine, the
    result is ``{"segments": [...], "diagnostics": {...}}`` so the host can log why a meeting came
    out with the speaker count it did.
    """
    if single_person:
        regions = vad(audio_path)
        segments = [
            {"start": region["start"], "end": region["end"], "speaker": "User", "source": "microphone"}
            for region in regions
        ]
        if progress:
            progress(1.0)
        return {
            "segments": segments,
            "diagnostics": {
                "engine": "voice-activity",
                "speakers": speaker_summary(segments),
                "settings": {},
                "audio_seconds": audio_seconds(audio_path),
            },
        }

    resolved = DiarizationSettings.from_env_and_params(settings)
    # Preferred: the torch-free ONNX engine, which can ship inside the app. pyannote.audio stays
    # supported for installs that already have it plus a local model.
    reasons: list[str] = []
    try:
        return diarize_onnx_with_diagnostics(audio_path, progress, resolved)
    except DiarizationUnavailable as error:
        reasons.append(str(error))
    try:
        segments = _diarize_pyannote(audio_path, progress)
        return {
            "segments": segments,
            "diagnostics": {
                # pyannote takes its own clustering defaults; the settings below do not apply to it.
                "engine": "pyannote",
                "speakers": speaker_summary(segments),
                "settings": {},
                "audio_seconds": audio_seconds(audio_path),
            },
        }
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

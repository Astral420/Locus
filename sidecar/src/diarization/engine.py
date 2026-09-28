"""Speaker diarization adapter. The packaged path uses the local pyannote model."""

from __future__ import annotations

import os
import wave
from collections.abc import Callable

from .vad import vad


def diarize(audio_path: str, single_person: bool = False, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, object]]:
    if single_person:
        regions = vad(audio_path)
        result = [{"start": region["start"], "end": region["end"], "speaker": "User", "source": "microphone"} for region in regions]
        if progress:
            progress(1.0)
        return result

    try:
        from pyannote.audio import Pipeline  # type: ignore
    except ImportError:
        result = _fallback_segments(audio_path)
        if progress:
            progress(1.0, "fallback diarization")
        return result

    model_path = os.environ.get("LOCUS_PYANNOTE_MODEL")
    if not model_path:
        result = _fallback_segments(audio_path)
        if progress:
            progress(1.0, "fallback diarization")
        return result
    pipeline = Pipeline.from_pretrained(model_path)
    annotation = pipeline(audio_path)
    result = [{"start": float(segment.start), "end": float(segment.end), "speaker": str(label), "source": "mixed"} for segment, _, label in annotation.itertracks(yield_label=True)]
    if progress:
        progress(1.0)
    return result


def _fallback_segments(audio_path: str) -> list[dict[str, object]]:
    """Return useful deterministic labels in minimal environments; never downloads a model."""
    regions = vad(audio_path)
    if len(regions) < 2:
        return [{"start": region["start"], "end": region["end"], "speaker": "SPEAKER_00", "source": "mixed"} for region in regions]
    return [{**region, "speaker": f"SPEAKER_{index % 2:02d}", "source": "mixed"} for index, region in enumerate(regions)]

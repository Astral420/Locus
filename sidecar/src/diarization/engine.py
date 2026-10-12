"""Speaker diarization adapter. The packaged path uses the local pyannote model."""

from __future__ import annotations

import os
from collections.abc import Callable

from .vad import vad


class DiarizationUnavailable(RuntimeError):
    """Raised when speaker diarization cannot run honestly in this install.

    The old behaviour fabricated alternating SPEAKER_00/SPEAKER_01 labels, which
    presents guesses as speaker identity. Callers must treat this as "no
    diarization", never as a reason to invent labels.
    """


def diarize(audio_path: str, single_person: bool = False, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, object]]:
    if single_person:
        regions = vad(audio_path)
        result = [{"start": region["start"], "end": region["end"], "speaker": "User", "source": "microphone"} for region in regions]
        if progress:
            progress(1.0)
        return result

    try:
        from pyannote.audio import Pipeline  # type: ignore
    except ImportError as error:
        raise DiarizationUnavailable("pyannote.audio is not installed") from error

    model_path = os.environ.get("LOCUS_PYANNOTE_MODEL")
    if not model_path:
        raise DiarizationUnavailable("no local pyannote model is configured (LOCUS_PYANNOTE_MODEL)")
    pipeline = Pipeline.from_pretrained(model_path)
    annotation = pipeline(audio_path)
    result = [{"start": float(segment.start), "end": float(segment.end), "speaker": str(label), "source": "mixed"} for segment, _, label in annotation.itertracks(yield_label=True)]
    if progress:
        progress(1.0)
    return result


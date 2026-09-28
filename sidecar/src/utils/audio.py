from __future__ import annotations

import wave
from pathlib import Path


def duration_seconds(audio_path: str | Path) -> float:
    with wave.open(str(audio_path), "rb") as audio:
        rate = audio.getframerate()
        return audio.getnframes() / rate if rate else 0.0

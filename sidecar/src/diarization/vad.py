"""Voice activity detection with pyannote when packaged, deterministic PCM fallback for development."""

from __future__ import annotations

import math
import wave
from collections.abc import Callable
from typing import Any


def vad(audio_path: str, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, float]]:
    try:
        from pyannote.audio import Pipeline  # type: ignore
    except ImportError:
        regions = _energy_vad(audio_path)
        if progress:
            progress(1.0, "energy fallback")
        return regions

    model_path = _model_path()
    if not model_path:
        regions = _energy_vad(audio_path)
        if progress:
            progress(1.0, "energy fallback")
        return regions
    pipeline = Pipeline.from_pretrained(model_path)
    annotation = pipeline(audio_path)
    regions = [{"start": float(segment.start), "end": float(segment.end)} for segment in annotation.get_timeline().support()]
    if progress:
        progress(1.0)
    return regions


def _model_path() -> str | None:
    import os
    value = os.environ.get("LOCUS_PYANNOTE_VAD_MODEL") or os.environ.get("LOCUS_PYANNOTE_MODEL")
    return value or None


def _energy_vad(audio_path: str, frame_ms: int = 30) -> list[dict[str, float]]:
    """Small stdlib fallback used for offline smoke tests when torch is not installed."""
    with wave.open(audio_path, "rb") as audio:
        rate, width, channels = audio.getframerate(), audio.getsampwidth(), audio.getnchannels()
        frames_per_window = max(1, int(rate * frame_ms / 1000))
        raw = audio.readframes(frames_per_window)
        active: list[tuple[float, float]] = []
        offset = 0
        current_start: float | None = None
        threshold = 0.015 if width == 2 else 1.0
        while raw:
            values = _samples(raw, width, channels)
            level = sum(abs(value) for value in values) / max(1, len(values))
            timestamp = offset / rate
            end = timestamp + len(values) / max(1, rate * channels)
            if level > threshold and current_start is None:
                current_start = timestamp
            if level <= threshold and current_start is not None:
                active.append((current_start, timestamp))
                current_start = None
            offset += frames_per_window
            raw = audio.readframes(frames_per_window)
        if current_start is not None:
            active.append((current_start, offset / rate))
    return [{"start": round(start, 6), "end": round(end, 6)} for start, end in _merge(active) if end > start]


def _samples(raw: bytes, width: int, channels: int) -> list[float]:
    if width == 2:
        values = [int.from_bytes(raw[index:index + 2], "little", signed=True) / 32768.0 for index in range(0, len(raw) - 1, 2)]
        return values
    return [float(byte - 128) / 128 for byte in raw]


def _merge(regions: list[tuple[float, float]], gap: float = 0.15) -> list[tuple[float, float]]:
    merged: list[tuple[float, float]] = []
    for region in regions:
        if merged and region[0] - merged[-1][1] <= gap:
            merged[-1] = (merged[-1][0], region[1])
        else:
            merged.append(region)
    return merged

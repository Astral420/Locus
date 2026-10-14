"""Speaker diarization without PyTorch.

Uses sherpa-onnx with the pyannote segmentation 3.0 model (MIT) and a speaker-embedding model, both
plain ONNX files. That keeps the whole feature to a small pip wheel plus about 47 MB of model files,
which can be bundled with the app (the pyannote.audio route needs PyTorch and a gated download).
"""

from __future__ import annotations

import os
import wave
from collections.abc import Callable

from ..errors import CapabilityUnavailable
from ..resources import find_model

SEGMENTATION_MODEL = ("diarization", "segmentation.onnx")
EMBEDDING_MODEL = ("diarization", "embedding.onnx")
SAMPLE_RATE = 16_000


class DiarizationUnavailable(CapabilityUnavailable):
    """Raised when speaker diarization cannot run honestly in this install.

    The old behaviour fabricated alternating SPEAKER_00/SPEAKER_01 labels, which presents guesses as
    speaker identity. Callers must treat this as "no diarization", never as a reason to invent labels.
    """


def relabel_by_first_appearance(raw_speakers: list[int]) -> list[str]:
    """Engine speaker ids are arbitrary (0, 3, ...); present them as SPEAKER_00, SPEAKER_01 in order of speaking."""
    order: dict[int, int] = {}
    for speaker in raw_speakers:
        order.setdefault(speaker, len(order))
    return [f"SPEAKER_{order[speaker]:02d}" for speaker in raw_speakers]


def read_mono_16k(audio_path: str):
    import numpy as np

    with wave.open(audio_path, "rb") as source:
        if source.getsampwidth() != 2:
            raise RuntimeError("speaker detection expects 16-bit PCM audio")
        rate, channels = source.getframerate(), source.getnchannels()
        raw = np.frombuffer(source.readframes(source.getnframes()), dtype=np.int16)
    samples = raw.astype(np.float32) / 32768.0
    if channels > 1:
        samples = samples[: len(samples) // channels * channels].reshape(-1, channels).mean(axis=1)
    if rate == SAMPLE_RATE:
        return samples
    if rate % SAMPLE_RATE == 0:
        factor = rate // SAMPLE_RATE
        return samples[: len(samples) // factor * factor].reshape(-1, factor).mean(axis=1)
    target = int(len(samples) * SAMPLE_RATE / rate)
    return np.interp(np.linspace(0, len(samples) - 1, target), np.arange(len(samples)), samples).astype(np.float32)


def diarize_onnx(audio_path: str, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, object]]:
    try:
        import sherpa_onnx  # type: ignore
    except ImportError as error:
        raise DiarizationUnavailable("sherpa-onnx is not installed (run scripts/setup-sidecar.sh)") from error
    segmentation, embedding = find_model(*SEGMENTATION_MODEL), find_model(*EMBEDDING_MODEL)
    if segmentation is None or embedding is None:
        raise DiarizationUnavailable(
            "the speaker-detection models were not found in models/diarization (run scripts/fetch-diarization-models.sh)"
        )

    threads = max(1, min(4, os.cpu_count() or 1))
    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(segmentation)),
            num_threads=threads,
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(embedding), num_threads=threads),
        clustering=sherpa_onnx.FastClusteringConfig(num_clusters=-1, threshold=0.5),
        min_duration_on=0.3,
        min_duration_off=0.5,
    )
    engine = sherpa_onnx.OfflineSpeakerDiarization(config)
    samples = read_mono_16k(audio_path)
    if engine.sample_rate != SAMPLE_RATE:  # the bundled models are 16 kHz; fail loudly rather than mislabel
        raise RuntimeError(f"unexpected diarization sample rate {engine.sample_rate}")

    def on_progress(done: int, total: int) -> int:
        if progress and total:
            progress(min(0.99, done / total), None)
        return 0

    found = engine.process(samples, callback=on_progress).sort_by_start_time()
    labels = relabel_by_first_appearance([int(segment.speaker) for segment in found])
    result = [
        {"start": float(segment.start), "end": float(segment.end), "speaker": label, "source": "mixed"}
        for segment, label in zip(found, labels)
    ]
    if progress:
        progress(1.0, None)
    return result

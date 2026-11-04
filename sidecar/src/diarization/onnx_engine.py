"""Speaker diarization without PyTorch.

Uses sherpa-onnx with the pyannote segmentation 3.0 model (MIT) and a speaker-embedding model, both
plain ONNX files. That keeps the whole feature to a small pip wheel plus about 47 MB of model files,
which can be bundled with the app (the pyannote.audio route needs PyTorch and a gated download).
"""

from __future__ import annotations

import math
import os
import wave
from collections.abc import Callable, Mapping
from dataclasses import asdict, dataclass, fields
from typing import Any

from ..errors import CapabilityUnavailable
from ..resources import find_model
from .postprocess import (
    absorb_tiny_speakers,
    merge_adjacent,
    relabel_by_first_appearance,
    speaker_summary,
)

SEGMENTATION_MODEL = ("diarization", "segmentation.onnx")
EMBEDDING_MODEL = ("diarization", "embedding.onnx")
SAMPLE_RATE = 16_000

# Speech under half a second is not enough to embed a voice; such slices are skipped instead of
# producing a noisy centroid.
MIN_CENTROID_SECONDS = 0.5
# A cluster below this share of all speech is treated as a stray label when a speaker floor is set.
TINY_SPEAKER_FRACTION = 0.03
# Diarization only needs consistent loudness, not fidelity; embedding models are sensitive to level.
TARGET_RMS_DBFS = -23.0
PEAK_CEILING = 0.95

ENV_SETTINGS = {
    "threshold": "LOCUS_DIARIZATION_THRESHOLD",
    "num_speakers": "LOCUS_DIARIZATION_NUM_SPEAKERS",
    "min_duration_on": "LOCUS_DIARIZATION_MIN_ON",
    "min_duration_off": "LOCUS_DIARIZATION_MIN_OFF",
}
DURATION_LIMITS = {
    "min_duration_on": (0.0, 5.0),
    "min_duration_off": (0.0, 5.0),
    "merge_gap": (0.0, 5.0),
    "min_speaker_seconds": (0.0, 7200.0),
}
MAX_SPEAKERS = 20


class DiarizationUnavailable(CapabilityUnavailable):
    """Raised when speaker diarization cannot run honestly in this install.

    The old behaviour fabricated alternating SPEAKER_00/SPEAKER_01 labels, which presents guesses as
    speaker identity. Callers must treat this as "no diarization", never as a reason to invent labels.
    """


@dataclass(frozen=True)
class DiarizationSettings:
    """Everything about clustering that used to be hard-coded.

    The defaults reproduce the shipped behaviour. ``merge_gap`` and ``min_speaker_seconds`` are off
    by default because they change labels; a build that has been measured against real recordings
    can turn them on (see ``scripts/diarization-sweep.py``).
    """

    # Clustering threshold: higher means two voices must be more similar to share a label.
    threshold: float = 0.5
    # A known head count beats clustering when the user (or a sweep) supplies one.
    num_speakers: int = -1
    min_duration_on: float = 0.3
    min_duration_off: float = 0.5
    # Join a speaker's consecutive segments separated by at most this many seconds.
    merge_gap: float = 0.0
    # Fold labels with less speech than this into an established speaker (0 disables).
    min_speaker_seconds: float = 0.0
    # Cosine similarity an absorbed label's centroid needs to a speaker to be folded into it.
    merge_similarity: float = 0.5

    @classmethod
    def from_env_and_params(
        cls,
        params: Mapping[str, Any] | None = None,
        environ: Mapping[str, str] | None = None,
    ) -> "DiarizationSettings":
        """Defaults, then environment variables, then RPC params (the last one wins).

        Values that cannot be parsed or are out of range fall back to the default rather than
        failing the whole step: a bad tuning value must never cost the user their speaker labels.
        """
        environ = os.environ if environ is None else environ
        params = params if isinstance(params, Mapping) else {}
        resolved: dict[str, Any] = {field.name: field.default for field in fields(cls)}
        for name, variable in ENV_SETTINGS.items():
            parsed = _parse_setting(name, environ.get(variable))
            if parsed is not None:
                resolved[name] = parsed
        for name in resolved:
            if name in params:
                parsed = _parse_setting(name, params[name])
                if parsed is not None:
                    resolved[name] = parsed
        return cls(**{name: _clamp_setting(name, value) for name, value in resolved.items()})

    def as_dict(self) -> dict[str, Any]:
        return asdict(self)


def _parse_setting(name: str, raw: Any) -> int | float | None:
    if raw is None or raw == "":
        return None
    try:
        value = float(raw)
    except (TypeError, ValueError):
        return None
    if math.isnan(value) or math.isinf(value):
        return None
    return int(value) if name == "num_speakers" else value


def _clamp_setting(name: str, value: int | float) -> int | float:
    if name == "threshold":
        return min(1.5, max(0.1, float(value)))
    if name == "num_speakers":
        number = int(value)
        if number <= 0:
            return -1  # automatic: let the clustering decide
        return min(MAX_SPEAKERS, number)
    if name == "merge_similarity":
        return min(1.0, max(0.0, float(value)))
    low, high = DURATION_LIMITS.get(name, (0.0, 5.0))
    return min(high, max(low, float(value)))


def audio_seconds(audio_path: str) -> float:
    with wave.open(audio_path, "rb") as source:
        rate = source.getframerate()
        return round(source.getnframes() / rate, 3) if rate else 0.0


# Anti-alias filter: passband to 7 kHz, stopband from 8 kHz (the model's Nyquist), 50 dB down.
# A 1 kHz transition is what a 16 kHz voice model needs: aliased content at 8-12 kHz lands right on
# top of the speech band and is what made two voices look like one, and one voice look like two.
ANTIALIAS_TRANSITION_HZ = 1000.0
ANTIALIAS_ATTENUATION_DB = 50.0


def _lowpass_branches(up: int, down: int, rate: int, target_rate: int = SAMPLE_RATE):
    """Kaiser-windowed sinc low-pass, split into one row of taps per polyphase branch.

    The cutoff sits between 7 kHz and the lower Nyquist, so content that would fold back into the
    speech band is removed before decimation instead of being averaged in by it.
    """
    import numpy as np

    upsampled_rate = rate * up
    band_edge = 0.5 * min(rate, target_rate)
    transition_hz = min(ANTIALIAS_TRANSITION_HZ, 0.5 * band_edge)
    cutoff = (band_edge - 0.5 * transition_hz) / upsampled_rate
    transition = transition_hz / upsampled_rate
    length = int(math.ceil((ANTIALIAS_ATTENUATION_DB - 8.0) / (2.285 * 2.0 * math.pi * transition))) + 1
    length += 1 - length % 2  # odd: an integer centre tap keeps the filter linear phase
    half = (length - 1) // 2
    index = np.arange(-half, half + 1, dtype=np.float64)
    taps = (2.0 * cutoff) * np.sinc(2.0 * cutoff * index) * np.kaiser(index.size, 8.6)
    # Exact unit gain per branch: a level shift would make the same voice look like a new speaker.
    taps *= up / np.sum(taps)
    padded = np.zeros(int(math.ceil(taps.size / up)) * up, dtype=np.float64)
    padded[: taps.size] = taps
    return padded.reshape(-1, up).T


def _resample_poly(samples, up: int, down: int, rate: int):
    """Band-limited rational resampling (gain-corrected) in pure numpy."""
    import numpy as np

    audio = np.asarray(samples, dtype=np.float32)
    if up == down or audio.size == 0:
        return audio
    branches = _lowpass_branches(up, down, rate)
    out_length = audio.size * up // down
    if out_length <= 0:
        return np.zeros(0, dtype=np.float32)
    # Breathing room on both sides so the filter window is always inside the array; the padding is
    # silence, which is what surrounds the file anyway.
    span = -(-branches.shape[1] // up)
    padded = np.concatenate(
        [np.zeros(span, dtype=np.float32), audio, np.zeros(span, dtype=np.float32)]
    )
    scaled = np.arange(out_length, dtype=np.int64) * down
    positions = scaled // up + span
    phases = scaled % up
    output = np.zeros(out_length, dtype=np.float64)
    for branch in range(branches.shape[1]):
        source_index = positions - branch
        valid = (source_index >= 0) & (source_index < padded.size)
        if not valid.any():
            continue
        output[valid] += branches[phases[valid], branch] * padded[source_index[valid]].astype(np.float64)
    return output.astype(np.float32)


def read_mono_16k(audio_path: str):
    """Read any 16-bit PCM WAV as mono float32 at 16 kHz, low-pass filtered before downsampling."""
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
    divisor = math.gcd(rate, SAMPLE_RATE)
    return _resample_poly(samples, SAMPLE_RATE // divisor, rate // divisor, rate)


def normalise_for_diarization(
    samples,
    target_dbfs: float = TARGET_RMS_DBFS,
    peak_ceiling: float = PEAK_CEILING,
):
    """Scale a *copy* to a consistent RMS level without clipping; the file on disk is untouched.

    Embeddings are extracted from absolute levels, so a quiet microphone track and a loud system
    track of the same voice read as different people. The peak ceiling keeps the gain from
    introducing the clipping that the old unconditional sum already suffered from.
    """
    import numpy as np

    audio = np.asarray(samples, dtype=np.float32)
    if audio.size == 0:
        return audio.copy()
    rms = float(np.sqrt(np.mean(np.square(audio, dtype=np.float64))))
    if rms <= 1e-6:
        return audio.copy()
    peak = float(np.max(np.abs(audio)))
    gain = 10.0 ** (target_dbfs / 20.0) / rms
    if peak > 0.0:
        gain = min(gain, peak_ceiling / peak)
    return (audio * gain).astype(np.float32)


def _speaker_centroids(
    sherpa_onnx,
    embedding_path: str,
    samples,
    segments: list[dict[str, Any]],
    threads: int,
) -> dict[str, Any]:
    """One averaged voice embedding per anonymous label, for deciding what a stray label belongs to."""
    import numpy as np

    try:
        extractor = sherpa_onnx.SpeakerEmbeddingExtractor(
            sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(embedding_path), num_threads=threads)
        )
        minimum = int(MIN_CENTROID_SECONDS * SAMPLE_RATE)
        buckets: dict[str, list[Any]] = {}
        for segment in segments:
            start = max(0, int(float(segment["start"]) * SAMPLE_RATE))
            end = min(samples.shape[0], int(float(segment["end"]) * SAMPLE_RATE))
            if end - start < minimum:
                continue
            stream = extractor.create_stream()
            stream.accept_waveform(SAMPLE_RATE, samples[start:end])
            stream.input_finished()
            if not extractor.is_ready(stream):
                continue
            buckets.setdefault(str(segment["speaker"]), []).append(
                np.asarray(extractor.compute(stream), dtype=np.float32)
            )
        return {
            label: np.mean(np.stack(vectors), axis=0)
            for label, vectors in buckets.items()
            if vectors
        }
    except Exception as error:  # an unusable extractor only loses the similarity tie-break
        print(f"locus: speaker embeddings unavailable for post-processing: {error}", flush=True)
        return {}


def diarize_onnx_with_diagnostics(
    audio_path: str,
    progress: Callable[[float, str | None], None] | None = None,
    settings: DiarizationSettings | None = None,
) -> dict[str, Any]:
    """Diarize a file and say how it went: segments plus per-speaker and settings diagnostics."""
    try:
        import sherpa_onnx  # type: ignore
    except ImportError as error:
        raise DiarizationUnavailable("sherpa-onnx is not installed (run scripts/setup-sidecar.sh)") from error
    segmentation, embedding = find_model(*SEGMENTATION_MODEL), find_model(*EMBEDDING_MODEL)
    if segmentation is None or embedding is None:
        raise DiarizationUnavailable(
            "the speaker-detection models were not found in models/diarization (run scripts/fetch-diarization-models.sh)"
        )

    settings = settings or DiarizationSettings.from_env_and_params()
    threads = max(1, min(4, os.cpu_count() or 1))
    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(segmentation)),
            num_threads=threads,
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(embedding), num_threads=threads),
        clustering=sherpa_onnx.FastClusteringConfig(
            # A supplied head count replaces the threshold: sherpa only clusters by threshold when
            # num_clusters is -1.
            num_clusters=settings.num_speakers if settings.num_speakers > 0 else -1,
            threshold=settings.threshold,
        ),
        min_duration_on=settings.min_duration_on,
        min_duration_off=settings.min_duration_off,
    )
    engine = sherpa_onnx.OfflineSpeakerDiarization(config)
    samples = read_mono_16k(audio_path)
    if engine.sample_rate != SAMPLE_RATE:  # the bundled models are 16 kHz; fail loudly rather than mislabel
        raise RuntimeError(f"unexpected diarization sample rate {engine.sample_rate}")
    # A copy at a consistent level: what the engine embeds, never what is stored on disk.
    prepared = normalise_for_diarization(samples)

    def on_progress(done: int, total: int) -> int:
        if progress and total:
            progress(min(0.99, done / total), None)
        return 0

    found = engine.process(prepared, callback=on_progress).sort_by_start_time()
    labels = relabel_by_first_appearance([int(segment.speaker) for segment in found])
    segments: list[dict[str, Any]] = [
        {"start": float(segment.start), "end": float(segment.end), "speaker": label, "source": "mixed"}
        for segment, label in zip(found, labels)
    ]

    if settings.min_speaker_seconds > 0 and len(segments) > 1:
        centroids = _speaker_centroids(sherpa_onnx, embedding, prepared, segments, threads)
        segments = absorb_tiny_speakers(
            segments,
            settings.min_speaker_seconds,
            TINY_SPEAKER_FRACTION,
            centroids,
            settings.merge_similarity,
        )
    segments = merge_adjacent(segments, settings.merge_gap)

    if progress:
        progress(1.0, None)
    return {
        "segments": segments,
        "diagnostics": {
            "engine": "sherpa-onnx",
            "speakers": speaker_summary(segments),
            "settings": settings.as_dict(),
            "audio_seconds": round(samples.shape[0] / SAMPLE_RATE, 3),
        },
    }


def diarize_onnx(
    audio_path: str,
    progress: Callable[[float, str | None], None] | None = None,
    settings: DiarizationSettings | None = None,
) -> list[dict[str, object]]:
    return diarize_onnx_with_diagnostics(audio_path, progress, settings)["segments"]

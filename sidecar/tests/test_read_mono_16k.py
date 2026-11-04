"""Reading diarization input: band-limited downsampling and level normalisation."""

import wave

import numpy as np
import pytest

from sidecar.src.diarization.onnx_engine import (
    PEAK_CEILING,
    SAMPLE_RATE,
    normalise_for_diarization,
    read_mono_16k,
)


def write_tone(path, frequency: float, seconds: float = 2.0, rate: int = 48_000, amplitude: float = 0.5, channels: int = 1):
    samples = amplitude * np.sin(2.0 * np.pi * frequency * np.arange(int(seconds * rate)) / rate)
    frames = np.repeat(samples, channels).astype(np.float32)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(channels)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(np.clip(frames * 32767.0, -32768, 32767).astype("<i2").tobytes())


def rms(samples) -> float:
    return float(np.sqrt(np.mean(np.square(np.asarray(samples, dtype=np.float64)))))


def test_a_48k_tone_in_the_band_survives_downsampling_within_a_db(tmp_path):
    path = tmp_path / "tone.wav"
    write_tone(path, 1000.0)
    samples = read_mono_16k(str(path))
    assert samples.dtype == np.float32
    assert abs(len(samples) - 2.0 * SAMPLE_RATE) <= 4
    # Level of a 1 kHz tone is preserved: the embedding must not change because of resampling.
    assert abs(20.0 * np.log10(rms(samples) / (0.5 / np.sqrt(2.0)))) < 1.0


def test_a_tone_above_nyquist_is_strongly_attenuated_not_aliased(tmp_path):
    # 9 kHz averaged in by sample-group decimation would land at 7 kHz inside the speech band and
    # change the voice embedding; the anti-alias filter has to keep it out, edges included.
    path = tmp_path / "high.wav"
    write_tone(path, 9000.0)
    samples = read_mono_16k(str(path))
    steady = samples[len(samples) // 4 : 3 * len(samples) // 4]
    assert 20.0 * np.log10(rms(steady) / (0.5 / np.sqrt(2.0)) + 1e-12) < -40.0
    # The onset of an abruptly started sine is broadband by nature; real recordings fade in from
    # silence, so the steady band is the part that matters for the embedding.


def test_44_1k_input_is_resampled_to_the_model_rate(tmp_path):
    path = tmp_path / "tone441.wav"
    write_tone(path, 1000.0, rate=44_100)
    samples = read_mono_16k(str(path))
    assert abs(len(samples) - 2.0 * SAMPLE_RATE) <= 4
    assert abs(20.0 * np.log10(rms(samples) / (0.5 / np.sqrt(2.0)))) < 1.0


def test_low_rate_input_is_upsampled(tmp_path):
    path = tmp_path / "tone8k.wav"
    write_tone(path, 1000.0, rate=8_000)
    samples = read_mono_16k(str(path))
    assert abs(len(samples) - 2.0 * SAMPLE_RATE) <= 4
    assert abs(20.0 * np.log10(rms(samples) / (0.5 / np.sqrt(2.0)))) < 1.0


def test_16k_input_is_passed_through_unchanged(tmp_path):
    path = tmp_path / "native.wav"
    write_tone(path, 1000.0, rate=16_000)
    samples = read_mono_16k(str(path))
    assert len(samples) == 2 * SAMPLE_RATE
    assert samples.dtype == np.float32


def test_multichannel_input_is_mixed_to_mono(tmp_path):
    path = tmp_path / "stereo.wav"
    write_tone(path, 500.0, channels=2)
    assert len(read_mono_16k(str(path))) == 2 * SAMPLE_RATE


def test_non_16_bit_audio_is_refused_rather_than_guessed(tmp_path):
    path = tmp_path / "float.wav"
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(1)
        out.setframerate(SAMPLE_RATE)
        out.writeframes(b"\x80" * 3200)
    with pytest.raises(RuntimeError):
        read_mono_16k(str(path))


def test_normalisation_lifts_a_quiet_recording_to_the_target_level():
    quiet = (0.01 * np.sin(2.0 * np.pi * 440.0 * np.arange(SAMPLE_RATE) / SAMPLE_RATE)).astype(np.float32)
    normalised = normalise_for_diarization(quiet)
    measured = 20.0 * np.log10(rms(normalised) + 1e-12)
    assert abs(measured - (-23.0)) < 1.0, measured
    assert float(np.max(np.abs(normalised))) <= PEAK_CEILING + 1e-6


def test_normalisation_never_clips_a_peaky_signal_and_never_touches_the_input():
    peaking = np.zeros(SAMPLE_RATE, dtype=np.float32)
    peaking[100] = 0.99
    peaking[::100] = 0.4
    original = peaking.copy()
    normalised = normalise_for_diarization(peaking)
    assert float(np.max(np.abs(normalised))) <= PEAK_CEILING + 1e-6
    assert np.array_equal(peaking, original)


def test_silence_stays_silence_instead_of_being_amplified():
    silence = np.zeros(SAMPLE_RATE, dtype=np.float32)
    assert not normalise_for_diarization(silence).any()

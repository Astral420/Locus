import wave
from pathlib import Path

from sidecar.src.diarization.engine import diarize
from sidecar.src.diarization.vad import vad


def write_fixture(path: Path) -> None:
    with wave.open(str(path), "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(8000)
        audio.writeframes((b"\x00\x00" * 800) + (b"\xff\x7f" * 1600) + (b"\x00\x00" * 800) + (b"\x00\x40" * 1600))


def test_vad_and_single_person_are_valid_without_network(tmp_path: Path):
    fixture = tmp_path / "speech.wav"
    write_fixture(fixture)
    progress = []
    regions = vad(str(fixture), lambda fraction, _message=None: progress.append(fraction))
    assert regions
    assert progress[-1] == 1.0
    segments = diarize(str(fixture), single_person=True)
    assert segments and {segment["speaker"] for segment in segments} == {"User"}


def test_empty_audio_is_a_valid_empty_result(tmp_path: Path):
    fixture = tmp_path / "empty.wav"
    with wave.open(str(fixture), "wb") as audio:
        audio.setnchannels(1); audio.setsampwidth(2); audio.setframerate(8000); audio.writeframes(b"\x00\x00" * 800)
    assert vad(str(fixture)) == []

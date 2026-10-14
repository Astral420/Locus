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


def _no_diarization_engine(monkeypatch):
    """Make this test independent of what happens to be installed on the machine."""
    import sys

    monkeypatch.delenv("LOCUS_PYANNOTE_MODEL", raising=False)
    monkeypatch.setattr("sidecar.src.diarization.onnx_engine.find_model", lambda *a: None)
    monkeypatch.setattr("sidecar.src.diarization.engine.find_model", lambda *a: None)
    monkeypatch.setitem(sys.modules, "pyannote", None)
    monkeypatch.setitem(sys.modules, "pyannote.audio", None)


def test_multi_speaker_diarization_never_invents_labels_without_a_model(tmp_path: Path, monkeypatch):
    import pytest

    from sidecar.src.diarization.engine import DiarizationUnavailable

    fixture = tmp_path / "speech.wav"
    write_fixture(fixture)
    _no_diarization_engine(monkeypatch)
    with pytest.raises(DiarizationUnavailable):
        diarize(str(fixture), single_person=False)


def test_rpc_reports_diarization_unavailable_instead_of_fake_speakers(tmp_path: Path, monkeypatch):
    from sidecar.src.rpc.server import JsonRpcServer

    fixture = tmp_path / "speech.wav"
    write_fixture(fixture)
    _no_diarization_engine(monkeypatch)
    server = JsonRpcServer.__new__(JsonRpcServer)
    result = server._diarize({"audio_path": str(fixture)}, type("Ctx", (), {"progress": staticmethod(lambda *a, **k: None)})())
    assert result["segments"] == []
    assert "unavailable" in result

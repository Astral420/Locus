"""Diarization and OCR engine selection. Heavy engines are exercised only where installed."""

import os
import shutil
import subprocess
import sys
import wave
from pathlib import Path

import pytest

from sidecar.src.diarization.engine import DiarizationUnavailable, diarize
from sidecar.src.diarization.onnx_engine import relabel_by_first_appearance
from sidecar.src.errors import CapabilityUnavailable
from sidecar.src.resources import find_model, model_dirs
from sidecar.src.slides import ocr as ocr_module


def _silence(path: Path, seconds: float = 1.0, rate: int = 16000) -> None:
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(b"\x00\x00" * int(seconds * rate))


# ---------------------------------------------------------------- resources


def test_models_are_found_through_the_host_supplied_directories(tmp_path, monkeypatch):
    first, second = tmp_path / "writable", tmp_path / "bundled"
    (second / "diarization").mkdir(parents=True)
    (second / "diarization" / "embedding.onnx").write_bytes(b"x")
    monkeypatch.setenv("LOCUS_MODEL_DIRS", os.pathsep.join([str(first), str(second)]))
    assert model_dirs()[:2] == [first, second]
    assert find_model("diarization", "embedding.onnx") == second / "diarization" / "embedding.onnx"
    assert find_model("diarization", "nope.onnx") is None


def test_a_frozen_bundle_looks_inside_its_own_models_folder(tmp_path, monkeypatch):
    monkeypatch.delenv("LOCUS_MODEL_DIRS", raising=False)
    (tmp_path / "models" / "diarization").mkdir(parents=True)
    (tmp_path / "models" / "diarization" / "segmentation.onnx").write_bytes(b"x")
    monkeypatch.setattr(sys, "_MEIPASS", str(tmp_path), raising=False)
    assert find_model("diarization", "segmentation.onnx") == tmp_path / "models" / "diarization" / "segmentation.onnx"


# ---------------------------------------------------------------- diarization


def test_speaker_ids_become_stable_labels_in_order_of_speaking():
    assert relabel_by_first_appearance([3, 0, 3, 0, 5]) == ["SPEAKER_00", "SPEAKER_01", "SPEAKER_00", "SPEAKER_01", "SPEAKER_02"]
    assert relabel_by_first_appearance([]) == []


def test_missing_models_mean_unavailable_never_invented_speakers(tmp_path, monkeypatch):
    pytest.importorskip("sherpa_onnx")
    monkeypatch.setenv("LOCUS_MODEL_DIRS", str(tmp_path / "empty"))
    monkeypatch.setattr("sidecar.src.diarization.onnx_engine.find_model", lambda *a: None)
    monkeypatch.setitem(sys.modules, "pyannote", None)
    monkeypatch.setitem(sys.modules, "pyannote.audio", None)
    audio = tmp_path / "a.wav"
    _silence(audio)
    with pytest.raises(DiarizationUnavailable) as error:
        diarize(str(audio), single_person=False)
    assert "models" in str(error.value)
    assert isinstance(error.value, CapabilityUnavailable)


def test_a_single_person_mic_needs_no_diarization_model(tmp_path):
    audio = tmp_path / "a.wav"
    _silence(audio)
    # silence has no speech regions; the point is that no engine is required and nothing is invented
    assert diarize(str(audio), single_person=True) == []


@pytest.mark.skipif(shutil.which("espeak-ng") is None, reason="needs espeak-ng to synthesise two voices")
def test_two_voices_are_told_apart_when_the_engine_and_models_are_installed(tmp_path):
    pytest.importorskip("sherpa_onnx")
    if find_model("diarization", "segmentation.onnx") is None or find_model("diarization", "embedding.onnx") is None:
        pytest.skip("diarization models not installed")
    turns = [
        ("en-us+m2", "Good morning everyone. Today we will review the architecture for the offline first desktop application."),
        ("en-us+f4", "Thanks. I have a question about the sidecar. Does it run on the CPU when the graphics card is not available?"),
        ("en-us+m2", "Yes, both whisper and the summarizer fall back cleanly to CPU instructions with bounded memory usage."),
        ("en-us+f4", "Great, that answers my concern. Let us move on to the slides."),
    ]
    parts = []
    for index, (voice, text) in enumerate(turns):
        part = tmp_path / f"t{index}.wav"
        subprocess.run(["espeak-ng", "-v", voice, "-s", "150", "-w", str(part), text], check=True)
        parts.append(part)
    mixed = tmp_path / "mix.wav"
    cmd = ["ffmpeg", "-loglevel", "error", "-y"]
    for part in parts:
        cmd += ["-i", str(part)]
    filters = "".join(f"[{i}]aresample=16000,apad=pad_dur=0.8[a{i}];" for i in range(len(parts)))
    cmd += ["-filter_complex", filters + "".join(f"[a{i}]" for i in range(len(parts))) + f"concat=n={len(parts)}:v=0:a=1", "-ac", "1", str(mixed)]
    subprocess.run(cmd, check=True)

    segments = diarize(str(mixed), single_person=False)
    labels = [segment["speaker"] for segment in segments]
    assert set(labels) == {"SPEAKER_00", "SPEAKER_01"}
    assert labels[0] == "SPEAKER_00"  # labels follow who speaks first
    # the voices alternate A B A B: same voice, same label; different voice, different label
    turn_labels = [labels[0]]
    for label in labels[1:]:
        if label != turn_labels[-1]:
            turn_labels.append(label)
    assert turn_labels == ["SPEAKER_00", "SPEAKER_01", "SPEAKER_00", "SPEAKER_01"]
    assert all(segment["start"] < segment["end"] for segment in segments)


# ---------------------------------------------------------------- OCR


class _FakeRapid:
    def __call__(self, image_path):
        return (
            [
                ([[0, 0]], "Quarterly Review", "0.99"),
                ([[0, 1]], "smudge", "0.31"),  # below the confidence floor
                ([[0, 2]], "  Next steps  ", "0.92"),
            ],
            None,
        )


def test_rapidocr_text_is_ordered_trimmed_and_filtered(tmp_path, monkeypatch):
    monkeypatch.setattr(ocr_module, "_engine", _FakeRapid())
    assert ocr_module.ocr_slide(str(tmp_path / "slide.png")) == "Quarterly Review\nNext steps"


def test_without_rapidocr_text_recognition_is_unavailable_not_a_crash(tmp_path, monkeypatch):
    monkeypatch.setattr(ocr_module, "_engine", None)
    monkeypatch.setitem(sys.modules, "rapidocr_onnxruntime", None)
    with pytest.raises(CapabilityUnavailable) as error:
        ocr_module.ocr_slide(str(tmp_path / "x.png"))
    assert "text recognition" in str(error.value)


def test_tesseract_is_not_a_fallback_even_when_it_is_installed(tmp_path, monkeypatch):
    class _Tesseract:
        def __getattr__(self, name):
            raise AssertionError("Tesseract must never be consulted")

    monkeypatch.setattr(ocr_module, "_engine", None)
    monkeypatch.setitem(sys.modules, "rapidocr_onnxruntime", None)
    monkeypatch.setitem(sys.modules, "pytesseract", _Tesseract())
    with pytest.raises(CapabilityUnavailable):
        ocr_module.ocr_slide(str(tmp_path / "x.png"))
    assert not hasattr(ocr_module, "_read_with_tesseract")


def test_real_rapidocr_reads_a_rendered_slide(tmp_path):
    pytest.importorskip("rapidocr_onnxruntime")
    pil = pytest.importorskip("PIL.Image")
    from PIL import ImageDraw, ImageFont

    font_path = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
    if not Path(font_path).exists():
        pytest.skip("no test font available")
    image = pil.new("RGB", (1280, 400), (255, 255, 255))
    ImageDraw.Draw(image).text((60, 120), "Quarterly Architecture Review", fill=(0, 0, 0), font=ImageFont.truetype(font_path, 64))
    slide = tmp_path / "slide.png"
    image.save(slide)
    ocr_module._engine = None
    text = ocr_module.ocr_slide(str(slide))
    assert "Architecture" in text and "Review" in text

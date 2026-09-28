import importlib.util
from pathlib import Path

import pytest


@pytest.mark.skipif(importlib.util.find_spec("cv2") is None, reason="opencv-python-headless is not installed")
def test_slide_detector_imports_with_opencv():
    from sidecar.src.slides.detector import detect_slides
    assert callable(detect_slides)


@pytest.mark.skipif(importlib.util.find_spec("cv2") is None, reason="opencv-python-headless is not installed")
def test_slide_detector_extracts_transition_frames(tmp_path: Path):
    import cv2
    import numpy as np

    video_path = tmp_path / "slides.avi"
    writer = cv2.VideoWriter(str(video_path), cv2.VideoWriter_fourcc(*"MJPG"), 10.0, (160, 90))
    assert writer.isOpened()
    for color in ((20, 20, 20), (200, 20, 20), (20, 200, 20), (20, 20, 200)):
        frame = np.full((90, 160, 3), color, dtype=np.uint8)
        for _ in range(10):
            writer.write(frame)
    writer.release()

    from sidecar.src.slides.detector import detect_slides

    slides = detect_slides(str(video_path), str(tmp_path / "out"))
    # Codec quantization can merge two adjacent solid-color transitions; the
    # detector must still emit multiple unique frames with valid artifacts.
    assert len(slides) >= 3
    assert [slide["ordinal"] for slide in slides] == list(range(len(slides)))
    assert [slide["timestamp"] for slide in slides] == sorted(slide["timestamp"] for slide in slides)
    assert all(Path(slide["image_path"]).is_file() for slide in slides)

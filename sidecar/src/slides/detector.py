"""OpenCV headless slide transition detector."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path


def detect_slides(video_path: str, output_dir: str | None = None, progress: Callable[[float, str | None], None] | None = None) -> list[dict[str, object]]:
    try:
        import cv2  # type: ignore
    except ImportError as exc:
        raise RuntimeError("opencv-python-headless is required for slide detection") from exc
    capture = cv2.VideoCapture(video_path)
    if not capture.isOpened():
        raise RuntimeError(f"unable to open video: {video_path}")
    fps = capture.get(cv2.CAP_PROP_FPS) or 30.0
    frame_count = capture.get(cv2.CAP_PROP_FRAME_COUNT) or 0.0
    threshold = 0.12
    destination = Path(output_dir) if output_dir else Path(video_path).parent / ".locus-slides"
    destination.mkdir(parents=True, exist_ok=True)
    slides: list[dict[str, object]] = []
    previous = None
    ordinal = 0
    index = 0
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
        small = cv2.resize(gray, (160, 90))
        if previous is None or float(cv2.absdiff(previous, small).mean()) / 255.0 > threshold:
            timestamp = index / fps
            image_path = destination / f"slide-{ordinal:04d}.png"
            if not cv2.imwrite(str(image_path), frame):
                raise RuntimeError(f"unable to write slide image: {image_path}")
            slides.append({"ordinal": ordinal, "timestamp": timestamp, "image_path": str(image_path), "transition": ordinal > 0})
            ordinal += 1
        previous = small
        index += 1
        if progress and frame_count:
            progress(index / frame_count)
    capture.release()
    if progress:
        progress(1.0)
    return slides

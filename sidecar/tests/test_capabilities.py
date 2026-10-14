import io
import sys

from sidecar.src.rpc.server import JsonRpcServer
from sidecar.src.errors import CAPABILITY_UNAVAILABLE


def _call(server, method, params):
    return server.dispatch({"jsonrpc": "2.0", "id": "1", "method": method, "params": params})


def test_missing_ocr_packages_are_reported_as_unavailable_not_as_a_crash(tmp_path, monkeypatch, caplog):
    image = tmp_path / "slide.png"
    image.write_bytes(b"not really an image")
    monkeypatch.setitem(sys.modules, "pytesseract", None)  # makes `import pytesseract` fail
    monkeypatch.setitem(sys.modules, "rapidocr_onnxruntime", None)
    monkeypatch.setattr("sidecar.src.slides.ocr._rapidocr_engine", None)
    server = JsonRpcServer(io.BytesIO(), io.BytesIO())
    with caplog.at_level("INFO", logger="locus.sidecar"):
        response = _call(server, "ocr_slide", {"image_path": str(image)})
    assert response["error"]["code"] == CAPABILITY_UNAVAILABLE
    assert "text recognition" in response["error"]["message"]
    # one INFO line, no ERROR record carrying a traceback
    assert not [r for r in caplog.records if r.levelname == "ERROR" or r.exc_info]


def test_missing_opencv_is_reported_as_unavailable(tmp_path, monkeypatch):
    video = tmp_path / "v.mp4"
    video.write_bytes(b"x")
    monkeypatch.setitem(sys.modules, "cv2", None)
    server = JsonRpcServer(io.BytesIO(), io.BytesIO())
    response = _call(server, "detect_slides", {"video_path": str(video)})
    assert response["error"]["code"] == CAPABILITY_UNAVAILABLE
    assert "opencv" in response["error"]["message"]


def test_other_errors_keep_their_own_codes():
    server = JsonRpcServer(io.BytesIO(), io.BytesIO())
    response = _call(server, "no_such_method", {})
    assert response["error"]["code"] != CAPABILITY_UNAVAILABLE

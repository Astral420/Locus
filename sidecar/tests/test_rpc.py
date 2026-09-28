import io
import json

from sidecar.src.rpc.server import JsonRpcServer


def test_handshake_health_and_protocol_errors():
    server = JsonRpcServer(io.BytesIO(), io.BytesIO())
    handshake = server.dispatch({"jsonrpc": "2.0", "id": "1", "method": "handshake", "params": {}})
    assert handshake["result"]["protocol_version"] == "2.0"
    health = server.dispatch({"jsonrpc": "2.0", "id": "2", "method": "health", "params": {}})
    assert health["result"]["ok"] is True
    assert server.dispatch({"jsonrpc": "2.0", "id": "3", "method": "missing", "params": {}})["error"]["code"] == -32601


def test_progress_is_newline_delimited_and_stdout_has_only_json():
    output = io.BytesIO()
    server = JsonRpcServer(io.BytesIO(), output)
    server._write_notification("progress", {"fraction": 0.5})
    lines = output.getvalue().splitlines()
    assert len(lines) == 1
    assert json.loads(lines[0])["method"] == "progress"

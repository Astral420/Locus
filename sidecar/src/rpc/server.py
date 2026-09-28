"""Bounded JSON-RPC 2.0 server for the Rust-owned sidecar process."""

from __future__ import annotations

import json
import logging
import os
import sys
from dataclasses import dataclass
from typing import Any, Callable

from ..diarization.engine import diarize
from ..diarization.vad import vad
from ..slides.detector import detect_slides
from ..slides.ocr import ocr_slide
from ..vectordb.store import VectorStore

MAX_FRAME_SIZE = 8 * 1024 * 1024
PROTOCOL_VERSION = "2.0"
logger = logging.getLogger("locus.sidecar")


class RpcFault(Exception):
    def __init__(self, code: int, message: str, data: Any = None) -> None:
        super().__init__(message)
        self.code, self.message, self.data = code, message, data


@dataclass
class RequestContext:
    job_id: str | None = None
    attempt_id: str | None = None
    emit: Callable[[str, dict[str, Any]], None] | None = None

    def progress(self, fraction: float, message: str | None = None) -> None:
        if self.emit:
            params: dict[str, Any] = {"fraction": max(0.0, min(1.0, fraction))}
            if self.job_id is not None:
                params["job_id"] = self.job_id
            if self.attempt_id is not None:
                params["attempt_id"] = self.attempt_id
            if message:
                params["message"] = message
            self.emit("progress", params)


class JsonRpcServer:
    def __init__(self, stdin=None, stdout=None, vector_store: VectorStore | None = None) -> None:
        self.stdin = stdin or sys.stdin.buffer
        self.stdout = stdout or sys.stdout.buffer
        self.vector_store = vector_store or VectorStore(os.environ.get("LOCUS_VECTOR_ROOT", ".locus-vectors"))
        self.cancelled: set[str] = set()
        self.methods: dict[str, Callable[[dict[str, Any], RequestContext], Any]] = {
            "handshake": self._handshake,
            "health": self._health,
            "vad": self._vad,
            "diarize": self._diarize,
            "detect_slides": self._detect_slides,
            "ocr_slide": self._ocr_slide,
            "upsert_vectors": self._upsert_vectors,
            "query_vectors": self._query_vectors,
            "delete_vectors": self._delete_vectors,
            "cancel": self._cancel,
        }

    def serve_forever(self) -> None:
        for raw in self.stdin:
            if len(raw) > MAX_FRAME_SIZE:
                self._write_error(None, -32600, "request frame exceeds maximum size")
                continue
            try:
                request = json.loads(raw.decode("utf-8"))
                response = self.dispatch(request)
            except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                self._write_error(None, -32700, f"parse error: {exc}")
                continue
            except Exception:
                logger.exception("unhandled sidecar request failure")
                continue
            if response is not None:
                self._write(response)

    def dispatch(self, request: dict[str, Any]) -> dict[str, Any] | None:
        if not isinstance(request, dict) or request.get("jsonrpc") != PROTOCOL_VERSION:
            return self._error(request.get("id") if isinstance(request, dict) else None, -32600, "invalid JSON-RPC request")
        method = request.get("method")
        request_id = request.get("id")
        if not isinstance(method, str):
            return self._error(request_id, -32600, "method is required")
        if request_id is None:
            return None
        params = request.get("params", {})
        if not isinstance(params, dict):
            return self._error(request_id, -32602, "params must be an object")
        handler = self.methods.get(method)
        if handler is None:
            return self._error(request_id, -32601, f"unknown method: {method}")
        context = RequestContext(params.get("job_id"), params.get("attempt_id"), self._write_notification)
        try:
            result = handler(params, context)
            return {"jsonrpc": PROTOCOL_VERSION, "result": result, "id": request_id}
        except RpcFault as exc:
            return self._error(request_id, exc.code, exc.message, exc.data)
        except FileNotFoundError as exc:
            return self._error(request_id, -32004, f"input not found: {exc}")
        except Exception as exc:
            logger.exception("method %s failed", method)
            return self._error(request_id, -32000, str(exc))

    def _handshake(self, _params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        return {"protocol_version": PROTOCOL_VERSION, "sidecar_version": "0.1.0", "capabilities": sorted(self.methods)}

    def _health(self, _params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        return {"ok": True, "vector_store": self.vector_store.health()}

    def _vad(self, params: dict[str, Any], context: RequestContext) -> dict[str, Any]:
        self._require_path(params, "audio_path")
        return {"regions": vad(params["audio_path"], progress=context.progress)}

    def _diarize(self, params: dict[str, Any], context: RequestContext) -> dict[str, Any]:
        self._require_path(params, "audio_path")
        return {"segments": diarize(params["audio_path"], bool(params.get("single_person", False)), progress=context.progress)}

    def _detect_slides(self, params: dict[str, Any], context: RequestContext) -> dict[str, Any]:
        self._require_path(params, "video_path")
        return {"slides": detect_slides(params["video_path"], params.get("output_dir"), progress=context.progress)}

    def _ocr_slide(self, params: dict[str, Any], context: RequestContext) -> dict[str, Any]:
        self._require_path(params, "image_path")
        context.progress(0.1)
        result = ocr_slide(params["image_path"])
        context.progress(1.0)
        return {"text": result}

    def _upsert_vectors(self, params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        self._require(params, "generation_id", "ids", "vectors", "documents", "metadatas")
        return self.vector_store.upsert(params["generation_id"], params["ids"], params["vectors"], params["documents"], params["metadatas"])

    def _query_vectors(self, params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        self._require(params, "generation_id", "query_vectors", "n_results")
        return self.vector_store.query(params["generation_id"], params["query_vectors"], int(params["n_results"]))

    def _delete_vectors(self, params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        self._require(params, "generation_id", "ids")
        return self.vector_store.delete(params["generation_id"], params["ids"])

    def _cancel(self, params: dict[str, Any], _context: RequestContext) -> dict[str, Any]:
        request_id = params.get("request_id")
        if not isinstance(request_id, str):
            raise RpcFault(-32602, "request_id is required")
        self.cancelled.add(request_id)
        return {"canceled": True, "request_id": request_id}

    @staticmethod
    def _require(params: dict[str, Any], *keys: str) -> None:
        missing = [key for key in keys if key not in params]
        if missing:
            raise RpcFault(-32602, f"missing params: {', '.join(missing)}")

    @staticmethod
    def _require_path(params: dict[str, Any], key: str) -> None:
        JsonRpcServer._require(params, key)
        path = params[key]
        if not isinstance(path, str) or not path:
            raise RpcFault(-32602, f"{key} must be a non-empty path")
        resolved = os.path.realpath(path)
        managed_root = os.environ.get("LOCUS_SIDECAR_ROOT")
        if managed_root:
            root = os.path.realpath(managed_root)
            try:
                if os.path.commonpath((resolved, root)) != root:
                    raise RpcFault(-32602, f"{key} is outside the managed sidecar root")
            except ValueError as exc:
                raise RpcFault(-32602, f"{key} is outside the managed sidecar root") from exc
        if not os.path.isfile(resolved):
            raise FileNotFoundError(path)

    def _write_notification(self, method: str, params: dict[str, Any]) -> None:
        self._write({"jsonrpc": PROTOCOL_VERSION, "method": method, "params": params})

    def _write_error(self, request_id: Any, code: int, message: str) -> None:
        self._write(self._error(request_id, code, message))

    @staticmethod
    def _error(request_id: Any, code: int, message: str, data: Any = None) -> dict[str, Any]:
        error: dict[str, Any] = {"code": code, "message": message}
        if data is not None:
            error["data"] = data
        return {"jsonrpc": PROTOCOL_VERSION, "error": error, "id": request_id}

    def _write(self, value: dict[str, Any]) -> None:
        encoded = (json.dumps(value, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")
        if len(encoded) > MAX_FRAME_SIZE:
            logger.error("response exceeded frame limit")
            return
        self.stdout.write(encoded)
        self.stdout.flush()

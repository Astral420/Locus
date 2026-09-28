"""Explicit-vector ChromaDB store with a deterministic no-dependency smoke-test backend."""

from __future__ import annotations

import json
import math
import os
import tempfile
import hashlib
from pathlib import Path
from typing import Any


def deterministic_chunk_id(source_id: str, source_revision: str, ordinal: int, chunker_version: str) -> str:
    payload = f"{source_id}\0{source_revision}\0{ordinal}\0{chunker_version}".encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


class VectorStore:
    def __init__(self, root: str | os.PathLike[str]) -> None:
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)
        self._chroma = None
        try:
            import chromadb  # type: ignore
            self._chroma = chromadb.PersistentClient(path=str(self.root))
        except ImportError:
            self._chroma = None

    def health(self) -> str:
        return "chromadb" if self._chroma is not None else "deterministic-fallback"

    def upsert(self, generation_id: str, ids: list[str], vectors: list[list[float]], documents: list[str], metadatas: list[dict[str, Any]]) -> dict[str, Any]:
        self._validate_lengths(ids, vectors, documents, metadatas)
        if self._chroma is not None:
            self._chroma.get_or_create_collection(generation_id).upsert(ids=ids, embeddings=vectors, documents=documents, metadatas=metadatas)
        else:
            data = self._load(generation_id)
            for item_id, vector, document, metadata in zip(ids, vectors, documents, metadatas):
                data[item_id] = {"id": item_id, "vector": vector, "document": document, "metadata": metadata}
            self._save(generation_id, data)
        return {"upserted": len(ids), "generation_id": generation_id}

    def query(self, generation_id: str, query_vectors: list[list[float]], n_results: int) -> dict[str, Any]:
        if n_results < 1:
            raise ValueError("n_results must be positive")
        if self._chroma is not None:
            result = self._chroma.get_or_create_collection(generation_id).query(query_embeddings=query_vectors, n_results=n_results)
            return {"ids": result.get("ids", []), "documents": result.get("documents", []), "metadatas": result.get("metadatas", []), "distances": result.get("distances", [])}
        data = list(self._load(generation_id).values())
        ids, documents, metadatas, distances = [], [], [], []
        for query in query_vectors:
            ranked = sorted(((self._distance(query, item["vector"]), item) for item in data), key=lambda pair: pair[0])[:n_results]
            ids.append([item["id"] for distance, item in ranked])
            documents.append([item["document"] for distance, item in ranked])
            metadatas.append([item["metadata"] for distance, item in ranked])
            distances.append([distance for distance, item in ranked])
        return {"ids": ids, "documents": documents, "metadatas": metadatas, "distances": distances}

    def delete(self, generation_id: str, ids: list[str]) -> dict[str, Any]:
        if self._chroma is not None:
            self._chroma.get_or_create_collection(generation_id).delete(ids=ids)
        else:
            data = self._load(generation_id)
            for item_id in ids:
                data.pop(item_id, None)
            self._save(generation_id, data)
        return {"deleted": len(ids), "generation_id": generation_id}

    def _path(self, generation_id: str) -> Path:
        safe = "".join(char if char.isalnum() or char in "-_" else "_" for char in generation_id)
        return self.root / f"{safe}.json"

    def _load(self, generation_id: str) -> dict[str, Any]:
        path = self._path(generation_id)
        return json.loads(path.read_text()) if path.exists() else {}

    def _save(self, generation_id: str, data: dict[str, Any]) -> None:
        path = self._path(generation_id)
        with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as handle:
            json.dump(data, handle, sort_keys=True)
            handle.flush()
            os.fsync(handle.fileno())
            temporary = Path(handle.name)
        temporary.replace(path)

    @staticmethod
    def _validate_lengths(*values: list[Any]) -> None:
        if not values or any(len(value) != len(values[0]) for value in values):
            raise ValueError("ids, vectors, documents and metadatas must have equal lengths")
        if len(set(len(vector) for vector in values[1])) > 1:
            raise ValueError("vectors must have equal dimensions")

    @staticmethod
    def _distance(first: list[float], second: list[float]) -> float:
        if len(first) != len(second):
            raise ValueError("query and stored vector dimensions differ")
        first_norm = math.sqrt(sum(value * value for value in first)) or 1.0
        second_norm = math.sqrt(sum(value * value for value in second)) or 1.0
        return 1.0 - sum(a * b for a, b in zip(first, second)) / (first_norm * second_norm)

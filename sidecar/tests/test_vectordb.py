from pathlib import Path

from sidecar.src.vectordb.store import VectorStore, deterministic_chunk_id


def test_vectors_upsert_query_delete_and_persist(tmp_path: Path):
    assert deterministic_chunk_id("source", "rev-1", 0, "chunker-1") == deterministic_chunk_id("source", "rev-1", 0, "chunker-1")
    store = VectorStore(tmp_path)
    store.upsert("generation-a", ["chunk-1", "chunk-2"], [[1.0, 0.0], [0.0, 1.0]], ["one", "two"], [{"source": "a"}, {"source": "b"}])
    result = store.query("generation-a", [[0.9, 0.1]], 1)
    assert result["ids"] == [["chunk-1"]]
    reloaded = VectorStore(tmp_path)
    assert reloaded.query("generation-a", [[0.0, 1.0]], 1)["ids"] == [["chunk-2"]]
    assert reloaded.delete("generation-a", ["chunk-1"])["deleted"] == 1
    assert reloaded.query("generation-a", [[1.0, 0.0]], 1)["ids"] == [["chunk-2"]]

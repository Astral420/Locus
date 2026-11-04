"""Post-processing for diarization segments: fewer labels, never invented ones.

The clustering step decides how many speakers exist from voice embeddings alone. On a summed
microphone/system mix that decision is noisy: one person often ends up with two labels, and a
cough, a chair creak or a sub-second sliver becomes a "third speaker" with two utterances. Neither
is evidence of a different person.

These helpers work on the plain segment dicts the engine returns (``start``, ``end``, ``speaker``,
``source``) and only ever *remove* labels: a stray cluster is folded into an existing speaker and
labels stay anonymous. They never split a speaker, and they never merge when the audio is
ambiguous, such as two voices talking over each other.
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from typing import Any

Segment = dict[str, Any]
Centroids = Mapping[str, Any]


def relabel_by_first_appearance(raw_speakers: Sequence[int]) -> list[str]:
    """Engine speaker ids are arbitrary (0, 3, ...); present them as SPEAKER_00, SPEAKER_01 in order of speaking."""
    order: dict[int, int] = {}
    for speaker in raw_speakers:
        order.setdefault(speaker, len(order))
    return [f"SPEAKER_{order[speaker]:02d}" for speaker in raw_speakers]


def _start(segment: Segment) -> float:
    return float(segment["start"])


def _end(segment: Segment) -> float:
    return float(segment["end"])


def _duration(segment: Segment) -> float:
    return max(0.0, _end(segment) - _start(segment))


def speaker_totals(segments: Sequence[Segment]) -> dict[str, float]:
    """Total talking time per anonymous label, in order of first appearance."""
    totals: dict[str, float] = {}
    for segment in segments:
        label = str(segment["speaker"])
        totals[label] = totals.get(label, 0.0) + _duration(segment)
    return totals


def speaker_summary(segments: Sequence[Segment]) -> list[dict[str, Any]]:
    """Per-speaker seconds and turn count, for diagnostics and the tuning sweep."""
    summaries: list[dict[str, Any]] = []
    index: dict[str, int] = {}
    for segment in segments:
        label = str(segment["speaker"])
        if label not in index:
            index[label] = len(summaries)
            summaries.append({"speaker": label, "seconds": 0.0, "segments": 0})
        entry = summaries[index[label]]
        entry["seconds"] = round(float(entry["seconds"]) + _duration(segment), 3)
        entry["segments"] = int(entry["segments"]) + 1
    return summaries


def turn_changes(segments: Sequence[Segment]) -> int:
    """How often the speaker label changes between consecutive segments."""
    labels = [str(segment["speaker"]) for segment in sorted(segments, key=_start)]
    return sum(1 for previous, current in zip(labels, labels[1:]) if previous != current)


def merge_adjacent(segments: Sequence[Segment], max_gap: float) -> list[Segment]:
    """Join consecutive segments of the same speaker separated by at most ``max_gap`` seconds.

    Clustering often returns one turn as several back-to-back segments around a breath. Joining
    them keeps the label count honest and gives each speaker one span per turn.
    """
    ordered = sorted(segments, key=lambda segment: (_start(segment), _end(segment)))
    merged: list[Segment] = []
    for segment in ordered:
        previous = merged[-1] if merged else None
        if (
            previous is not None
            and previous["speaker"] == segment["speaker"]
            and previous.get("source") == segment.get("source")
            and _start(segment) - _end(previous) <= max(0.0, max_gap)
        ):
            previous["end"] = max(_end(previous), _end(segment))
        else:
            merged.append(dict(segment))
    return merged


def _cosine_similarity(left: Any, right: Any) -> float:
    import numpy as np

    left_vector = np.asarray(left, dtype=np.float64).ravel()
    right_vector = np.asarray(right, dtype=np.float64).ravel()
    if left_vector.size == 0 or left_vector.shape != right_vector.shape:
        return -1.0
    denominator = float(np.linalg.norm(left_vector) * np.linalg.norm(right_vector))
    if denominator <= 0.0:
        return -1.0
    return float(np.dot(left_vector, right_vector) / denominator)


def _overlap_seconds(segments: Sequence[Segment], first: str, second: str) -> float:
    shared = 0.0
    for left in segments:
        if str(left["speaker"]) != first:
            continue
        for right in segments:
            if str(right["speaker"]) != second:
                continue
            shared += max(0.0, min(_end(left), _end(right)) - max(_start(left), _start(right)))
    return shared


def _temporal_distance(segments: Sequence[Segment], first: str, second: str) -> float:
    """Smallest distance in seconds between the two speakers' speech, ignoring simultaneous parts."""
    first_spans = [(_start(s), _end(s)) for s in segments if str(s["speaker"]) == first]
    second_spans = [(_start(s), _end(s)) for s in segments if str(s["speaker"]) == second]
    best = float("inf")
    for a_start, a_end in first_spans:
        for b_start, b_end in second_spans:
            distance = max(0.0, max(a_start, b_start) - min(a_end, b_end))
            best = min(best, distance)
    return best


def _closest_by_centroid(
    label: str,
    candidates: Sequence[str],
    centroids: Centroids | None,
    merge_similarity: float,
) -> str | None:
    """The real speaker whose averaged voice embedding is closest, if any is close enough."""
    if not centroids or label not in centroids:
        return None
    best: str | None = None
    best_score = -2.0
    for candidate in candidates:
        if candidate not in centroids:
            continue
        score = _cosine_similarity(centroids[label], centroids[candidate])
        if score > best_score:
            best, best_score = candidate, score
    return best if best is not None and best_score >= merge_similarity else None


def absorb_tiny_speakers(
    segments: Sequence[Segment],
    min_seconds: float,
    min_fraction: float,
    centroids: Centroids | None = None,
    merge_similarity: float = 0.5,
) -> list[Segment]:
    """Fold a label with almost no speech into a speaker that is demonstrably present.

    A speaker below ``max(min_seconds, min_fraction * total speech)`` may be one utterance, a
    cough, or bleed from the other track: too little audio for a trustworthy voice embedding, and
    no evidence of a separate person. The label is reassigned to the closest established speaker by
    averaged-embedding similarity, or failing that to the speaker talking nearest in time.

    Two cases are deliberately left alone: audio that only has one other speaker *and* overlaps it
    (simultaneous speech is a different question than a stray cluster), and a tiny label with a
    clearly distinct voice (see the centroid threshold), which is a real person who said little.
    """
    totals = speaker_totals(segments)
    if len(totals) < 2 or (min_seconds <= 0.0 and min_fraction <= 0.0):
        return merge_adjacent(segments, 0.0)

    floor = max(float(min_seconds), float(min_fraction) * sum(totals.values()))
    established = [label for label, seconds in totals.items() if seconds >= floor]
    strays = [label for label, seconds in totals.items() if seconds < floor]
    if not established or not strays:
        return merge_adjacent(segments, 0.0)

    first_seen: dict[str, float] = {}
    for segment in segments:
        label = str(segment["speaker"])
        first_seen[label] = min(first_seen.get(label, _start(segment)), _start(segment))

    # Smallest first, so a two-utterance cluster is resolved before a merely short one.
    mapping: dict[str, str] = {}
    for label in sorted(strays, key=lambda name: (totals[name], first_seen[name])):
        remaining = [candidate for candidate in established if candidate not in mapping]
        if not remaining:
            break
        # A label that overlaps its only alternative was talking at the same time as it: that is
        # not a stray copy of the same person, so leave the labels apart.
        if len(remaining) == 1 and _overlap_seconds(segments, label, remaining[0]) > 0.5 * totals[label]:
            continue
        comparable = [candidate for candidate in remaining if candidate in (centroids or {})]
        if label in (centroids or {}) and comparable:
            # The voices can be compared: only fold into a speaker this one actually resembles. A
            # short label whose voice matches nobody is a quiet participant, not a stray cluster.
            target = _closest_by_centroid(label, comparable, centroids, merge_similarity)
            if target is None:
                continue
        else:
            # No usable embeddings: fall back to who was talking nearest in time.
            target = min(
                remaining,
                key=lambda candidate: (_temporal_distance(segments, label, candidate), established.index(candidate)),
            )
        mapping[label] = target

    if not mapping:
        return merge_adjacent(segments, 0.0)

    absorbed = [
        {**segment, "speaker": mapping.get(str(segment["speaker"]), str(segment["speaker"]))}
        for segment in sorted(segments, key=lambda segment: (_start(segment), _end(segment)))
    ]
    # Labels keep the "in order of speaking" convention after the merge.
    ordered: list[str] = []
    for segment in absorbed:
        label = str(segment["speaker"])
        if label not in ordered:
            ordered.append(label)
    indices = {label: position for position, label in enumerate(ordered)}
    labels = relabel_by_first_appearance([indices[str(segment["speaker"])] for segment in absorbed])
    return merge_adjacent(
        [{**segment, "speaker": label} for segment, label in zip(absorbed, labels)], 0.0
    )

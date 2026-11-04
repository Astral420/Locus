"""Post-processing that removes stray speaker labels without inventing anyone."""

from sidecar.src.diarization.postprocess import (
    absorb_tiny_speakers,
    merge_adjacent,
    speaker_summary,
    turn_changes,
)


def segment(speaker: str, start: float, end: float, source: str = "mixed") -> dict:
    return {"start": start, "end": end, "speaker": speaker, "source": source}


def test_a_short_stray_between_two_turns_of_one_speaker_disappears():
    # SPEAKER_01 says 1.2 s in the middle of SPEAKER_00's thirty second turn, with no overlap: that
    # is one cluster created from too little audio, not a third person.
    segments = [
        segment("SPEAKER_00", 0.0, 12.0),
        segment("SPEAKER_01", 12.5, 13.7),
        segment("SPEAKER_00", 14.0, 30.0),
    ]
    result = absorb_tiny_speakers(segments, min_seconds=4.0, min_fraction=0.03)
    assert {item["speaker"] for item in result} == {"SPEAKER_00"}
    assert result[0]["start"] == 0.0 and result[-1]["end"] == 30.0


def test_two_genuinely_distinct_speakers_stay():
    segments = [
        segment("SPEAKER_00", 0.0, 15.0),
        segment("SPEAKER_01", 15.5, 30.0),
    ]
    result = absorb_tiny_speakers(segments, min_seconds=4.0, min_fraction=0.03)
    assert [item["speaker"] for item in result] == ["SPEAKER_00", "SPEAKER_01"]


def test_a_quiet_but_distinct_voice_keeps_its_own_label():
    # 3 s of a 30 min meeting is under the floor, but its voice embedding is clearly its own: a real
    # person who said one sentence, not a stray cluster.
    segments = [segment("SPEAKER_00", 0.0, 1770.0), segment("SPEAKER_01", 1800.0, 1803.0)]
    centroids = {"SPEAKER_00": [1.0, 0.0, 0.0], "SPEAKER_01": [0.0, 1.0, 0.0]}
    result = absorb_tiny_speakers(
        segments, min_seconds=4.0, min_fraction=0.03, centroids=centroids, merge_similarity=0.5
    )
    assert {item["speaker"] for item in result} == {"SPEAKER_00", "SPEAKER_01"}


def test_a_stray_whose_centroid_matches_is_folded_into_that_speaker():
    segments = [
        segment("SPEAKER_00", 0.0, 20.0),
        segment("SPEAKER_01", 20.5, 30.0),
        segment("SPEAKER_01", 60.0, 61.0),
    ]
    # SPEAKER_01 is short overall; its centroid points at SPEAKER_00's voice.
    centroids = {"SPEAKER_00": [0.9, 0.1, 0.0], "SPEAKER_01": [0.88, 0.12, 0.0]}
    result = absorb_tiny_speakers(
        segments, min_seconds=12.0, min_fraction=0.03, centroids=centroids, merge_similarity=0.5
    )
    assert {item["speaker"] for item in result} == {"SPEAKER_00"}
    assert [item["speaker"] for item in result] == ["SPEAKER_00"] * 3


def test_simultaneous_speech_is_never_merged_into_its_only_neighbour():
    # The only other speaker talks over the tiny label for most of its duration: two voices at once
    # is a coverage question for the caller, not evidence that the label is a copy.
    segments = [
        segment("SPEAKER_00", 0.0, 60.0),
        segment("SPEAKER_01", 10.0, 11.5),
    ]
    result = absorb_tiny_speakers(segments, min_seconds=4.0, min_fraction=0.03)
    assert {item["speaker"] for item in result} == {"SPEAKER_00", "SPEAKER_01"}


def test_labels_keep_the_order_of_first_speaking_after_a_merge():
    segments = [
        segment("SPEAKER_02", 0.0, 10.0),
        segment("SPEAKER_00", 10.4, 11.0),
        segment("SPEAKER_01", 11.5, 25.0),
    ]
    centroids = {"SPEAKER_02": [1.0, 0.0], "SPEAKER_00": [1.0, 0.0], "SPEAKER_01": [0.0, 1.0]}
    result = absorb_tiny_speakers(
        segments, min_seconds=5.0, min_fraction=0.0, centroids=centroids, merge_similarity=0.5
    )
    assert [item["speaker"] for item in result] == ["SPEAKER_00", "SPEAKER_00", "SPEAKER_01"]


def test_merge_gap_boundaries_inclusive_and_speaker_scoped():
    segments = [
        segment("SPEAKER_00", 0.0, 1.0),
        segment("SPEAKER_00", 1.8, 2.5),  # gap 0.8: merged at max_gap 0.8
        segment("SPEAKER_00", 3.4, 4.0),  # gap 0.9: kept apart
        segment("SPEAKER_01", 4.1, 5.0),
        segment("SPEAKER_01", 4.2, 5.5),  # overlapping duplicate turn
    ]
    result = merge_adjacent(segments, 0.8)
    assert [(item["speaker"], item["start"], item["end"]) for item in result] == [
        ("SPEAKER_00", 0.0, 2.5),
        ("SPEAKER_00", 3.4, 4.0),
        ("SPEAKER_01", 4.1, 5.5),
    ]
    assert len(merge_adjacent(segments, 0.0)) == 4


def test_merge_never_crosses_sources_or_speakers():
    segments = [
        segment("SPEAKER_00", 0.0, 1.0, source="microphone"),
        segment("SPEAKER_00", 1.1, 2.0, source="system"),
        segment("SPEAKER_01", 2.1, 3.0, source="system"),
    ]
    assert len(merge_adjacent(segments, 1.0)) == 3


def test_summary_and_turn_changes_measure_what_the_sweep_reports():
    segments = [
        segment("SPEAKER_00", 0.0, 2.0),
        segment("SPEAKER_01", 2.0, 3.5),
        segment("SPEAKER_00", 3.5, 4.0),
    ]
    assert speaker_summary(segments) == [
        {"speaker": "SPEAKER_00", "seconds": 2.5, "segments": 2},
        {"speaker": "SPEAKER_01", "seconds": 1.5, "segments": 1},
    ]
    assert turn_changes(segments) == 2
    assert turn_changes([]) == 0


def test_no_floor_means_no_absorption():
    segments = [segment("SPEAKER_00", 0.0, 20.0), segment("SPEAKER_01", 20.5, 21.0)]
    result = absorb_tiny_speakers(segments, min_seconds=0.0, min_fraction=0.0)
    assert {item["speaker"] for item in result} == {"SPEAKER_00", "SPEAKER_01"}

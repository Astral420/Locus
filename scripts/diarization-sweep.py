#!/usr/bin/env python3
"""Measure how the diarization grid splits one recording, so its defaults can be chosen from evidence.

    .venv/bin/python scripts/diarization-sweep.py path/to/audio.wav [--expected-speakers 2]

Runs every combination of clustering threshold, minimum turn length and minimum silence and prints,
per run: how many speakers came out, how long each of them talks, how many of them are too short to
be a real participant, and how often the speaker changes. With ``--expected-speakers`` the runs that
produce that head count are marked and the most stable one is recommended.

This reads a WAV with the same code the sidecar uses and prints a table; it needs no ground-truth
file and adds no dependency beyond numpy and sherpa-onnx, which the sidecar already requires.
"""

from __future__ import annotations

import argparse
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from sidecar.src.diarization.onnx_engine import (  # noqa: E402
    DiarizationSettings,
    DiarizationUnavailable,
    diarize_onnx_with_diagnostics,
)
from sidecar.src.diarization.postprocess import turn_changes  # noqa: E402

THRESHOLDS = (0.5, 0.6, 0.7, 0.8, 0.9)
MIN_ON = (0.3, 0.6, 1.0)
MIN_OFF = (0.5, 0.8, 1.2)
SHORT_SPEAKER_SECONDS = 5.0


def seconds_of(segments) -> list[float]:
    totals: dict[str, float] = {}
    for segment in segments:
        totals[str(segment["speaker"])] = totals.get(str(segment["speaker"]), 0.0) + (
            float(segment["end"]) - float(segment["start"])
        )
    return sorted(totals.values(), reverse=True)


def run(audio: str, settings: DiarizationSettings) -> dict:
    result = diarize_onnx_with_diagnostics(audio, None, settings)
    lengths = seconds_of(result["segments"])
    return {
        "settings": settings,
        "speakers": len(lengths),
        "lengths": lengths,
        "short": sum(1 for seconds in lengths if seconds < SHORT_SPEAKER_SECONDS),
        "turns": turn_changes(result["segments"]),
        "audio_seconds": result["diagnostics"]["audio_seconds"],
    }


def describe(row: dict) -> str:
    lengths = "/".join(f"{seconds:.0f}" for seconds in row["lengths"])
    return (
        f"{row['settings'].threshold:>4.1f} {row['settings'].min_duration_on:>6.1f} "
        f"{row['settings'].min_duration_off:>7.1f} {row['speakers']:>8} {lengths or '-':>26} "
        f"{row['short']:>10} {row['turns']:>8}"
    )


def choose(rows: list[dict], expected: int | None) -> tuple[dict, str]:
    """Most stable run: the head count that keeps reappearing, with the fewest stray short labels."""
    if expected is not None:
        candidates = [row for row in rows if row["speakers"] == expected]
        if not candidates:
            counts = Counter(row["speakers"] for row in rows)
            spread = ", ".join(f"{count} speaker(s) in {runs} run(s)" for count, runs in sorted(counts.items()))
            return (
                min(rows, key=lambda row: (abs(row["speakers"] - expected), row["short"])),
                f"no combination produced exactly {expected} speakers ({spread}); the closest is shown",
            )
    else:
        counts = Counter(row["speakers"] for row in rows)
        expected = counts.most_common(1)[0][0]
        candidates = [row for row in rows if row["speakers"] == expected]

    # A plateau: the same head count appears for neighbouring settings. Prefer the middle of it, and
    # among those the run with the fewest labels too short to be a participant.
    def stability(row: dict) -> tuple[int, int, float]:
        same = sum(
            1
            for other in candidates
            if other["speakers"] == row["speakers"]
            and abs(other["settings"].threshold - row["settings"].threshold) < 0.11
        )
        return (-same, row["short"], row["settings"].threshold)

    best = sorted(candidates, key=stability)[0]
    return best, f"recommended from a plateau of {len(candidates)} run(s) with {expected} speaker(s)"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("audio", help="16-bit PCM WAV, mono or stereo, any sample rate")
    parser.add_argument("--expected-speakers", type=int, default=None, help="head count you know the recording has")
    parser.add_argument("--thresholds", default=",".join(str(value) for value in THRESHOLDS))
    parser.add_argument("--min-on", default=",".join(str(value) for value in MIN_ON))
    parser.add_argument("--min-off", default=",".join(str(value) for value in MIN_OFF))
    parser.add_argument("--min-speaker-seconds", type=float, default=0.0, help="also absorb labels shorter than this")
    parser.add_argument("--merge-gap", type=float, default=0.0, help="also join a speaker's segments this close")
    args = parser.parse_args(argv)

    grid = [
        (float(threshold), float(min_on), float(min_off))
        for threshold in args.thresholds.split(",")
        for min_on in args.min_on.split(",")
        for min_off in args.min_off.split(",")
    ]
    print(f"audio: {args.audio}")
    print(
        f"grid: {len(grid)} run(s); threshold x min_duration_on x min_duration_off; "
        f"min_speaker_seconds={args.min_speaker_seconds} merge_gap={args.merge_gap}"
    )
    print(
        f"\n{'thr':>4} {'min_on':>6} {'min_off':>7} {'speakers':>8} {'seconds per speaker':>26} "
        f"{'under 5s':>10} {'turns':>8}"
    )
    print("-" * 78)

    rows: list[dict] = []
    for threshold, min_on, min_off in grid:
        settings = DiarizationSettings(
            threshold=threshold,
            min_duration_on=min_on,
            min_duration_off=min_off,
            min_speaker_seconds=args.min_speaker_seconds,
            merge_gap=args.merge_gap,
        )
        try:
            row = run(args.audio, settings)
        except DiarizationUnavailable as error:
            print(f"\nthe diarization models or sherpa-onnx are missing: {error}", file=sys.stderr)
            return 2
        except Exception as error:  # one bad combination must not hide the rest of the grid
            print(f"{threshold:>4.1f} {min_on:>6.1f} {min_off:>7.1f}   failed: {error}")
            continue
        rows.append(row)
        marker = "  <- matches" if args.expected_speakers is not None and row["speakers"] == args.expected_speakers else ""
        print(describe(row) + marker)

    if not rows:
        print("\nno combination ran; nothing to recommend", file=sys.stderr)
        return 1

    best, reason = choose(rows, args.expected_speakers)
    settings = best["settings"]
    print(
        f"\nreccomendation: threshold={settings.threshold:g} min_duration_on={settings.min_duration_on:g} "
        f"min_duration_off={settings.min_duration_off:g} -> {best['speakers']} speaker(s), "
        f"{best['short']} under {SHORT_SPEAKER_SECONDS:g}s ({reason})"
    )
    print(
        "apply it with LOCUS_DIARIZATION_THRESHOLD / _MIN_ON / _MIN_OFF, or per call as the "
        '"settings" RPC parameter.'
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

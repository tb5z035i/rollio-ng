#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pyarrow>=15",
#   "pandas>=2.1",
#   "av>=12",
#   "numpy>=1.24",
# ]
# ///
"""
Count encoded frames per channel in a LeRobot v2.1 episode.

For the parquet timeline, this is the number of rows (one row per nominal frame).
For each declared ``video`` feature, this counts frames in the corresponding MP4.

Example::

    uv run lerobot_episode_frame_counts.py output/d435i-airbot-play-eef-sprint5 --episode 0
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import av

from extract_episode_frame import DatasetInfo


def count_mp4_video_frames(path: Path) -> int:
    """Return the number of video frames in ``path``.

    Uses ``Stream.frames`` when the container reports it; otherwise decodes and
    counts (slower but reliable).
    """
    with av.open(str(path)) as container:
        if not container.streams.video:
            raise RuntimeError(f"No video stream in {path}")
        stream = container.streams.video[0]
        n = int(stream.frames or 0)
        if n > 0:
            return n
        return sum(1 for _ in container.decode(stream))


def main() -> int:
    p = argparse.ArgumentParser(
        description="Count frames per channel (parquet row + each video) in a LeRobot v2.1 episode.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument(
        "dataset_root",
        type=Path,
        help="Dataset root: directory that contains meta/info.json (e.g. your output folder).",
    )
    p.add_argument(
        "-e",
        "--episode",
        type=int,
        default=0,
        help="Episode index (matches episode_NNNNNN in data/videos paths).",
    )
    p.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON instead of a table.",
    )
    args = p.parse_args()

    info = DatasetInfo.load(args.dataset_root)
    pq_path = info.parquet_path(args.episode)
    if not pq_path.is_file():
        print(f"error: parquet not found: {pq_path}", file=sys.stderr)
        return 2

    import pyarrow.parquet as pq

    table = pq.read_table(pq_path)
    row_count = table.num_rows

    video_keys = info.video_keys()
    counts: dict[str, int] = {"parquet": row_count}

    for key in video_keys:
        vp = info.video_path(key, args.episode)
        if not vp.is_file():
            counts[key] = -1
            continue
        try:
            counts[key] = count_mp4_video_frames(vp)
        except Exception as exc:
            print(f"warning: {key}: could not read {vp}: {exc}", file=sys.stderr)
            counts[key] = -1

    if args.json:
        print(
            json.dumps(
                {
                    "dataset_root": str(args.dataset_root.resolve()),
                    "episode_index": args.episode,
                    "parquet_path": str(pq_path),
                    "frame_counts": counts,
                },
                indent=2,
            )
        )
        return 0

    w = max(len(k) for k in counts)
    w = max(w, len("channel"))
    print(f"dataset: {args.dataset_root.resolve()}")
    print(f"episode: {args.episode}")
    print()
    print(f"{'channel':<{w}}  frames")
    print(f"{'-' * w}  ------")
    for name in ["parquet", *[k for k in counts if k != "parquet"]]:
        n = counts[name]
        cell = "MISSING" if n < 0 else str(n)
        print(f"{name:<{w}}  {cell}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

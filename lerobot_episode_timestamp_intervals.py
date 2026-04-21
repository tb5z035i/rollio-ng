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
Measure per-channel intervals between consecutive frame timestamps in a LeRobot v2.1 episode.

* **Parquet** uses the ``timestamp`` column (rows ordered by ``frame_index`` when present).
* **Video** channels decode each frame and use FFmpeg/PyAV presentation time (``frame.time``,
  seconds) between consecutive decoded frames.

Reports min, max, and average interval in seconds. With fewer than two frames in a channel,
interval statistics are undefined (reported as null / \"n/a\").

Example::

    uv run lerobot_episode_timestamp_intervals.py output/d435i-airbot-play-eef-sprint5 --episode 0
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from extract_episode_frame import DatasetInfo


def stats_seconds(intervals_s: np.ndarray) -> dict[str, Any]:
    """Return min / max / mean over positive-length interval arrays; empty -> nulls."""
    if intervals_s.size == 0:
        return {"min_s": None, "max_s": None, "avg_s": None, "count_intervals": 0}
    return {
        "min_s": float(intervals_s.min()),
        "max_s": float(intervals_s.max()),
        "avg_s": float(intervals_s.mean()),
        "count_intervals": int(intervals_s.size),
    }


def parquet_timestamp_intervals(df: pd.DataFrame) -> np.ndarray:
    if "frame_index" in df.columns:
        df = df.sort_values("frame_index", kind="mergesort")
    ts = df["timestamp"].to_numpy(dtype=np.float64)
    if ts.size < 2:
        return np.array([], dtype=np.float64)
    return np.diff(ts)


def video_presentation_intervals_seconds(path: Path) -> np.ndarray:
    """Consecutive differences of ``frame.time`` (seconds) in decode order."""
    import av

    times: list[float] = []
    with av.open(str(path)) as container:
        if not container.streams.video:
            raise RuntimeError(f"No video stream in {path}")
        stream = container.streams.video[0]
        for frame in container.decode(stream):
            t = frame.time
            if t is None:
                continue
            times.append(float(t))

    if len(times) < 2:
        return np.array([], dtype=np.float64)
    a = np.asarray(times, dtype=np.float64)
    return np.diff(a)


def main() -> int:
    p = argparse.ArgumentParser(
        description="Per-channel timestamp interval stats (min/max/avg) for a LeRobot v2.1 episode.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument(
        "dataset_root",
        type=Path,
        help="Dataset root: directory that contains meta/info.json.",
    )
    p.add_argument("-e", "--episode", type=int, default=0, help="Episode index.")
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

    df = pq.read_table(pq_path).to_pandas()
    if df.empty:
        print("error: episode parquet is empty", file=sys.stderr)
        return 2
    if "timestamp" not in df.columns:
        print("error: missing 'timestamp' column in parquet", file=sys.stderr)
        return 2

    channels: dict[str, dict[str, Any]] = {}
    channels["parquet.timestamp"] = stats_seconds(parquet_timestamp_intervals(df))

    for key in info.video_keys():
        vp = info.video_path(key, args.episode)
        if not vp.is_file():
            channels[key] = {
                "min_s": None,
                "max_s": None,
                "avg_s": None,
                "count_intervals": 0,
                "error": "video file missing",
            }
            continue
        try:
            iv = video_presentation_intervals_seconds(vp)
            channels[key] = stats_seconds(iv)
        except Exception as exc:
            channels[key] = {
                "min_s": None,
                "max_s": None,
                "avg_s": None,
                "count_intervals": 0,
                "error": str(exc),
            }

    if args.json:
        print(
            json.dumps(
                {
                    "dataset_root": str(args.dataset_root.resolve()),
                    "episode_index": args.episode,
                    "parquet_path": str(pq_path),
                    "channels": channels,
                },
                indent=2,
                default=str,
            )
        )
        return 0

    def fmt_num(x: Any) -> str:
        if x is None or (isinstance(x, float) and (math.isnan(x) or math.isinf(x))):
            return "n/a"
        return f"{float(x):.9f}"

    print(f"dataset: {args.dataset_root.resolve()}")
    print(f"episode: {args.episode}")
    print()
    hdr = f"{'channel':<40}  {'nΔ':>8}  {'min_s':>14}  {'max_s':>14}  {'avg_s':>14}"
    print(hdr)
    print("-" * len(hdr))

    order = ["parquet.timestamp", *[k for k in channels if k != "parquet.timestamp"]]
    for name in order:
        ch = channels[name]
        n = ch.get("count_intervals", 0)
        err = ch.get("error")
        extra = f"  ({err})" if err else ""
        print(
            f"{name:<40}  {n:>8}  {fmt_num(ch.get('min_s')):>14}  "
            f"{fmt_num(ch.get('max_s')):>14}  {fmt_num(ch.get('avg_s')):>14}{extra}"
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

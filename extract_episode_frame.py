#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pyarrow>=15",
#   "pandas>=2.1",
#   "av>=12",
#   "pillow>=10",
#   "numpy>=1.24",
# ]
# ///
"""
Extract a single frame's worth of data from a recorded LeRobot v2.1 episode.

Given a dataset root (the directory containing ``meta/info.json``) plus an
episode index and a frame selector (timestamp, relative time, frame index, or
global index), this script:

  * locates the matching row in the episode's parquet file,
  * dumps that row as JSON (lists for array-valued columns), and
  * decodes the corresponding frame from each ``video`` feature into PNG.

Run directly with uv:

    uv run extract_episode_frame.py output/d435i-airbot-play-eef-sprint5 \\
        --episode 0 --frame-index 50 --out /tmp/frame50

Frame selection (pick exactly one):

    --timestamp T     Match the row whose ``timestamp`` column is closest to T.
                      Works both for absolute (wall-clock) and relative
                      (zero-based) timestamps, since matching is done in the
                      parquet's own timestamp space.
    --time T          Same as --timestamp but interpreted as seconds since the
                      episode start (subtracts the first-row timestamp before
                      matching). Convenient for rollio outputs that store
                      wall-clock UNIX seconds.
    --frame-index N   Match by the per-episode ``frame_index`` column.
    --index N         Match by the dataset-global ``index`` (or
                      ``global_index``) column.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

# ---------------------------------------------------------------------------
# Dataset metadata


DEFAULT_DATA_TPL = "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet"
DEFAULT_VIDEO_TPL = "videos/chunk-{chunk_index:03d}/{video_key}/episode_{episode_index:06d}.mp4"


def _ensure_templated(tpl: str, default: str, required: tuple[str, ...]) -> str:
    """Return ``tpl`` if it contains every required placeholder, else ``default``.

    Some recorders (notably rollio's ``output/meta/info.json``) store a literal
    path for the first episode rather than a real template; using such a string
    with ``str.format`` silently ignores ``--episode`` and ``--video-key``,
    causing every call to resolve to the same file.
    """
    if all("{" + name in tpl for name in required):
        return tpl
    return default


@dataclass
class DatasetInfo:
    root: Path
    fps: int
    chunks_size: int
    data_path_tpl: str
    video_path_tpl: str
    features: dict[str, dict[str, Any]]
    data_path_overridden: bool = False
    video_path_overridden: bool = False

    @classmethod
    def load(cls, root: Path) -> DatasetInfo:
        info_path = root / "meta" / "info.json"
        if not info_path.is_file():
            raise FileNotFoundError(f"meta/info.json not found under {root}")
        info = json.loads(info_path.read_text())

        raw_data = info.get("data_path", DEFAULT_DATA_TPL)
        raw_video = info.get("video_path", DEFAULT_VIDEO_TPL)
        data_tpl = _ensure_templated(raw_data, DEFAULT_DATA_TPL, ("episode_index",))
        video_tpl = _ensure_templated(raw_video, DEFAULT_VIDEO_TPL, ("episode_index", "video_key"))
        return cls(
            root=root,
            fps=int(info.get("fps", 30)),
            chunks_size=int(info.get("chunks_size", 1000)),
            data_path_tpl=data_tpl,
            video_path_tpl=video_tpl,
            features=info.get("features", {}),
            data_path_overridden=data_tpl != raw_data,
            video_path_overridden=video_tpl != raw_video,
        )

    def chunk_index(self, episode_index: int) -> int:
        return episode_index // self.chunks_size

    def parquet_path(self, episode_index: int) -> Path:
        rel = self.data_path_tpl.format(
            chunk_index=self.chunk_index(episode_index),
            episode_index=episode_index,
        )
        return self.root / rel

    def video_path(self, video_key: str, episode_index: int) -> Path:
        """Resolve the on-disk mp4 for ``video_key``.

        LeRobot canonical layout uses the full feature key as ``{video_key}``
        (e.g. ``observation.images.front``). Rollio's writer strips the
        ``observation.images.`` prefix and uses just ``front``. We try the
        canonical name first, then sensible aliases, and return the first
        candidate that exists. If none exist, return the canonical path so the
        caller can report a clean error.
        """
        candidates: list[str] = [video_key]
        for prefix in ("observation.images.", "observation.image.", "observation."):
            if video_key.startswith(prefix):
                candidates.append(video_key[len(prefix) :])
        seen: set[str] = set()
        canonical: Path | None = None
        for cand in candidates:
            if cand in seen:
                continue
            seen.add(cand)
            rel = self.video_path_tpl.format(
                chunk_index=self.chunk_index(episode_index),
                episode_index=episode_index,
                video_key=cand,
            )
            path = self.root / rel
            if canonical is None:
                canonical = path
            if path.is_file():
                return path
        assert canonical is not None
        return canonical

    def video_keys(self) -> list[str]:
        return [name for name, spec in self.features.items() if spec.get("dtype") == "video"]


# ---------------------------------------------------------------------------
# Frame selection


def select_row(
    df: pd.DataFrame,
    *,
    timestamp: float | None,
    rel_time: float | None,
    frame_index: int | None,
    global_index: int | None,
) -> tuple[int, str]:
    """Return ``(row_position, description)`` for the chosen selector."""
    selectors = [timestamp, rel_time, frame_index, global_index]
    if sum(s is not None for s in selectors) != 1:
        raise ValueError(
            "Exactly one of --timestamp / --time / --frame-index / --index must be provided."
        )

    if frame_index is not None:
        mask = df["frame_index"].to_numpy() == frame_index
        hits = np.flatnonzero(mask)
        if hits.size == 0:
            raise LookupError(
                f"frame_index={frame_index} not found in episode "
                f"(range: {int(df['frame_index'].min())}..{int(df['frame_index'].max())})"
            )
        return int(hits[0]), f"frame_index={frame_index}"

    if global_index is not None:
        idx_col = "index" if "index" in df.columns else "global_index"
        if idx_col not in df.columns:
            raise LookupError("Neither 'index' nor 'global_index' column present in parquet.")
        mask = df[idx_col].to_numpy() == global_index
        hits = np.flatnonzero(mask)
        if hits.size == 0:
            raise LookupError(
                f"{idx_col}={global_index} not found "
                f"(range: {int(df[idx_col].min())}..{int(df[idx_col].max())})"
            )
        return int(hits[0]), f"{idx_col}={global_index}"

    ts = df["timestamp"].to_numpy(dtype=np.float64)
    target = float(timestamp) if timestamp is not None else float(rel_time) + ts[0]
    pos = int(np.argmin(np.abs(ts - target)))
    label = "timestamp" if timestamp is not None else "time"
    val = timestamp if timestamp is not None else rel_time
    return pos, f"{label}={val} -> closest ts={ts[pos]:.6f} (Δ={ts[pos] - target:+.6f}s)"


# ---------------------------------------------------------------------------
# Row -> JSON


def row_to_jsonable(row: pd.Series) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, val in row.items():
        if isinstance(val, np.ndarray):
            out[key] = val.tolist()
        elif isinstance(val, list | tuple):
            out[key] = [v.item() if isinstance(v, np.generic) else v for v in val]
        elif isinstance(val, np.generic):
            out[key] = val.item()
        elif isinstance(val, float) and (math.isnan(val) or math.isinf(val)):
            out[key] = None if math.isnan(val) else str(val)
        else:
            out[key] = val
    return out


# ---------------------------------------------------------------------------
# Video frame extraction


def extract_video_frame(video_path: Path, seek_seconds: float) -> np.ndarray:
    """Decode the frame at (or just after) ``seek_seconds`` and return RGB ndarray."""
    import av

    seek_seconds = max(0.0, float(seek_seconds))
    with av.open(str(video_path)) as container:
        if not container.streams.video:
            raise RuntimeError(f"No video stream in {video_path}")
        stream = container.streams.video[0]

        time_base = stream.time_base or Fraction(1, 1000)
        seek_pts = int(seek_seconds / time_base)
        try:
            container.seek(seek_pts, any_frame=False, backward=True, stream=stream)
        except av.AVError:
            container.seek(0)

        chosen = None
        for frame in container.decode(stream):
            if frame.time is None:
                continue
            if frame.time + 1e-6 >= seek_seconds:
                chosen = frame
                break
            chosen = frame

        if chosen is None:
            raise RuntimeError(
                f"Could not decode any frame at t={seek_seconds:.6f}s in {video_path}"
            )
        return chosen.to_ndarray(format="rgb24")


# ---------------------------------------------------------------------------
# CLI


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Extract a frame + parquet row from a LeRobot v2.1 episode.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument(
        "dataset_root",
        type=Path,
        help="Path to the dataset root (directory with meta/info.json).",
    )
    p.add_argument("-e", "--episode", type=int, default=0, help="Episode index.")

    sel = p.add_argument_group("frame selector (choose exactly one)")
    sel.add_argument(
        "-t",
        "--timestamp",
        type=float,
        default=None,
        help="Match by parquet 'timestamp' column (absolute).",
    )
    sel.add_argument("-T", "--time", type=float, default=None, help="Seconds since episode start.")
    sel.add_argument(
        "-f", "--frame-index", type=int, default=None, help="Per-episode frame_index value."
    )
    sel.add_argument(
        "-i", "--index", type=int, default=None, help="Dataset-global index / global_index value."
    )

    p.add_argument(
        "-o",
        "--out",
        type=Path,
        default=None,
        help="Output directory (created if missing). Default: ./extracted_ep<EE>_f<FFFFFF>",
    )
    p.add_argument(
        "--video-key",
        action="append",
        default=None,
        help="Restrict to specific video feature(s); repeatable. "
        "Default: all video features in info.json.",
    )
    p.add_argument("--no-video", action="store_true", help="Skip video frame extraction.")
    p.add_argument(
        "--print-row", action="store_true", help="Also pretty-print the JSON row to stdout."
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()
    info = DatasetInfo.load(args.dataset_root)

    if info.data_path_overridden:
        print(
            "warning: meta/info.json 'data_path' is not a template "
            "(missing {episode_index}); falling back to default LeRobot v2.1 "
            f"layout: {DEFAULT_DATA_TPL}",
            file=sys.stderr,
        )
    if info.video_path_overridden:
        print(
            "warning: meta/info.json 'video_path' is not a template "
            "(missing {episode_index}/{video_key}); falling back to default "
            f"LeRobot v2.1 layout: {DEFAULT_VIDEO_TPL}",
            file=sys.stderr,
        )

    parquet_path = info.parquet_path(args.episode)
    if not parquet_path.is_file():
        print(f"error: parquet not found: {parquet_path}", file=sys.stderr)
        return 2

    df = pq.read_table(parquet_path).to_pandas()
    if df.empty:
        print(f"error: episode {args.episode} parquet is empty", file=sys.stderr)
        return 2

    try:
        row_pos, sel_desc = select_row(
            df,
            timestamp=args.timestamp,
            rel_time=args.time,
            frame_index=args.frame_index,
            global_index=args.index,
        )
    except (ValueError, LookupError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    row = df.iloc[row_pos]
    frame_idx = int(row["frame_index"])
    ts_abs = float(row["timestamp"])
    ts_rel = ts_abs - float(df["timestamp"].iloc[0])

    out_dir = args.out or Path(f"extracted_ep{args.episode:02d}_f{frame_idx:06d}")
    out_dir.mkdir(parents=True, exist_ok=True)

    row_json = row_to_jsonable(row)
    row_json["_meta"] = {
        "dataset_root": str(args.dataset_root),
        "episode_index": args.episode,
        "selector": sel_desc,
        "row_position_in_parquet": row_pos,
        "timestamp_absolute": ts_abs,
        "timestamp_relative_s": ts_rel,
        "fps": info.fps,
    }
    data_path = out_dir / "data.json"
    data_path.write_text(json.dumps(row_json, indent=2, default=str))

    print(f"selected: {sel_desc}")
    print(f"  episode_index={args.episode} frame_index={frame_idx} row={row_pos}/{len(df) - 1}")
    print(f"  timestamp(abs)={ts_abs:.6f}  rel={ts_rel:.6f}s")
    print(f"wrote {data_path}")

    if args.print_row:
        print("--- row ---")
        print(json.dumps(row_json, indent=2, default=str))

    if args.no_video:
        return 0

    keys = args.video_key if args.video_key else info.video_keys()
    if not keys:
        print("(no video features declared in info.json; skipping)")
        return 0

    from PIL import Image

    for key in keys:
        vp = info.video_path(key, args.episode)
        if not vp.is_file():
            print(f"  [skip] {key}: missing {vp}")
            continue
        try:
            arr = extract_video_frame(vp, ts_rel)
        except Exception as exc:
            print(f"  [error] {key}: {exc}")
            continue
        safe_key = key.replace("/", "_").replace(".", "_")
        png_path = out_dir / f"{safe_key}.png"
        Image.fromarray(arr).save(png_path)
        print(f"  [ok] {key}: {png_path}  ({arr.shape[1]}x{arr.shape[0]})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

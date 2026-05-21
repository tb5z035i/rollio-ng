#!/usr/bin/env bash
# Regenerate the vendored .bfbs files from the in-tree .fbs sources.
#
# Requires:
#   - flatc (FlatBuffers compiler, version 25.12.19)
#
# Env overrides:
#   FLATC   path to flatc binary (default: flatc on PATH)

set -Eeuo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FLATC="${FLATC:-flatc}"
OUTPUT_DIR="$REPO_ROOT/bfbs"

FOXGLOVE_FBS="$REPO_ROOT/utils/schemas/foxglove"
DISCOVER_FBS="$REPO_ROOT/utils/schemas/discover"

SCHEMAS=(
    "$FOXGLOVE_FBS/CompressedVideo.fbs"
    "$FOXGLOVE_FBS/RawImage.fbs"
    "$FOXGLOVE_FBS/JointStates.fbs"
    "$FOXGLOVE_FBS/CameraCalibration.fbs"
    "$FOXGLOVE_FBS/FrameTransform.fbs"
    "$DISCOVER_FBS/Imu.fbs"
    "$DISCOVER_FBS/TactileData.fbs"
)

die() { printf '\033[1;31m[regenerate-bfbs]\033[0m %s\n' "$*" >&2; exit 1; }
log() { printf '\033[1;34m[regenerate-bfbs]\033[0m %s\n' "$*" >&2; }

command -v "$FLATC" >/dev/null 2>&1 || die "flatc not found. Install FlatBuffers 25.12.19"
log "Using: $($FLATC --version)"

[[ -d "$FOXGLOVE_FBS" ]] || die "Foxglove schemas not found at $FOXGLOVE_FBS"
[[ -d "$DISCOVER_FBS" ]] || die "Discover schemas not found at $DISCOVER_FBS"

for fbs in "${SCHEMAS[@]}"; do
    [[ -f "$fbs" ]] || die "Schema not found: $fbs"
done

mkdir -p "$OUTPUT_DIR"

log "Compiling ${#SCHEMAS[@]} schemas to $OUTPUT_DIR"
"$FLATC" --schema -b \
    -I "$FOXGLOVE_FBS" \
    -I "$DISCOVER_FBS" \
    -o "$OUTPUT_DIR" \
    "${SCHEMAS[@]}"

log "Done. Generated files:"
ls -1 "$OUTPUT_DIR"/*.bfbs

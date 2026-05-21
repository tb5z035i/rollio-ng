#!/usr/bin/env bash
# Verify that all required .bfbs schema files are present and non-empty.
# Exits 0 on success, 1 if any file is missing or empty.

set -Eeuo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BFBS_DIR="${1:-$REPO_ROOT/bfbs}"

REQUIRED=(
    CompressedVideo.bfbs
    RawImage.bfbs
    JointStates.bfbs
    Imu.bfbs
    TactileData.bfbs
    CameraCalibration.bfbs
    FrameTransform.bfbs
)

ok=0
fail=0

for f in "${REQUIRED[@]}"; do
    path="$BFBS_DIR/$f"
    if [[ ! -f "$path" ]]; then
        printf '\033[1;31mMISSING\033[0m %s\n' "$path"
        fail=$((fail + 1))
    elif [[ ! -s "$path" ]]; then
        printf '\033[1;33mEMPTY\033[0m   %s\n' "$path"
        fail=$((fail + 1))
    else
        printf '\033[1;32mOK\033[0m      %s (%d bytes)\n' "$f" "$(stat -c%s "$path")"
        ok=$((ok + 1))
    fi
done

echo
if ((fail > 0)); then
    printf '%d/%d checks failed\n' "$fail" "${#REQUIRED[@]}"
    exit 1
else
    printf 'All %d schemas present and non-empty\n' "$ok"
fi

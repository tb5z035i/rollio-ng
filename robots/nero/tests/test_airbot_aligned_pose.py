"""Calibration regression for `rollio_device_nero.airbot_aligned_pose`.

Six physical EE orientations were jig-matched on hardware and recorded
on both the Nero and AIRBOT Play drivers. The recorded quaternions
(scalar-last `xyzw` for both, matching the `Pose7` wire format) are
locked in here as the canonical fit for the orientation half of the
adapter.

Both quaternion conventions agree at the codebase level (see the docs
in `airbot_aligned_pose.py` for the full trace), so this is a
*physical* calibration encoding two independent constants:

  * `Q_BASE_FIX_XYZW`: world / base-mounting rotation between the two
    arms (a 180-degree yaw on this rig).
  * `Q_TCP_FIX_XYZW`: TCP frame difference between the two driver
    URDF conventions.

Position alignment uses `Q_BASE_FIX_XYZW` only -- TCP origins are
assumed coincident -- so the position-side regression checks that
`(x, y, z) -> (-x, -y, z)` on this rig.

If this fixture changes, the constants in `airbot_aligned_pose.py`
need to be re-fit from the new data.
"""

from __future__ import annotations

import math

import pytest

from rollio_device_nero.airbot_aligned_pose import (
    Q_BASE_FIX_XYZW,
    Q_TCP_FIX_XYZW,
    apply_command_pose_fix,
    apply_publish_pose_fix,
)


# Each tuple is `(q_nero_xyzw, q_airbot_xyzw)` for the same physical
# orientation. The rounded-to-3dp values come straight from the
# operator-recorded driver outputs; the helper uses a true 1/sqrt(2)
# constant so the predicted quaternions match these to ~1e-3.
SQRT2_OVER_2 = math.sqrt(2.0) / 2.0
HALF = 0.5

CALIBRATION_PAIRS: tuple[
    tuple[
        tuple[float, float, float, float],
        tuple[float, float, float, float],
    ],
    ...,
] = (
    # 1) Nero R_y(-90 deg) <-> Airbot identity
    ((0.0, -SQRT2_OVER_2, 0.0, SQRT2_OVER_2), (0.0, 0.0, 0.0, 1.0)),
    # 2) Nero (-1/2, -1/2, 1/2, 1/2) <-> Airbot R_x(+90 deg)
    ((-HALF, -HALF, HALF, HALF), (SQRT2_OVER_2, 0.0, 0.0, SQRT2_OVER_2)),
    # 3) Nero (1/2, -1/2, -1/2, 1/2) <-> Airbot R_x(-90 deg)
    ((HALF, -HALF, -HALF, HALF), (-SQRT2_OVER_2, 0.0, 0.0, SQRT2_OVER_2)),
    # 4) Nero identity <-> Airbot R_y(-90 deg)
    ((0.0, 0.0, 0.0, 1.0), (0.0, -SQRT2_OVER_2, 0.0, SQRT2_OVER_2)),
    # 5) Nero R_z(+90 deg) <-> Airbot (1/2, -1/2, 1/2, 1/2)
    ((0.0, 0.0, SQRT2_OVER_2, SQRT2_OVER_2), (HALF, -HALF, HALF, HALF)),
    # 6) Nero R_z(-90 deg) <-> Airbot (1/2, 1/2, 1/2, -1/2)
    ((0.0, 0.0, -SQRT2_OVER_2, SQRT2_OVER_2), (HALF, HALF, HALF, -HALF)),
)

_TOL = 1e-6


def _quat_close_xyzw(
    a: tuple[float, float, float, float] | list[float],
    b: tuple[float, float, float, float] | list[float],
    tol: float = _TOL,
) -> bool:
    """`q` and `-q` represent the same rotation, so accept either sign."""
    pos = sum((float(ai) - float(bi)) ** 2 for ai, bi in zip(a, b))
    neg = sum((float(ai) + float(bi)) ** 2 for ai, bi in zip(a, b))
    return min(pos, neg) ** 0.5 < tol


def _wrap_pose(q: tuple[float, float, float, float]) -> list[float]:
    """Padding `(x, y, z) = (0, 0, 0)` so we can re-use the Pose7 helpers."""
    return [0.0, 0.0, 0.0, q[0], q[1], q[2], q[3]]


@pytest.mark.parametrize(
    "q_nero, q_airbot",
    CALIBRATION_PAIRS,
    ids=[f"pose-{idx + 1}" for idx in range(len(CALIBRATION_PAIRS))],
)
def test_publish_fix_maps_nero_to_airbot_within_calibration(
    q_nero: tuple[float, float, float, float],
    q_airbot: tuple[float, float, float, float],
) -> None:
    fixed = apply_publish_pose_fix(_wrap_pose(q_nero))
    assert _quat_close_xyzw(tuple(fixed[3:7]), q_airbot, tol=1e-3), (
        f"publish-fix on Nero {q_nero} produced {fixed[3:7]}, "
        f"expected ~ {q_airbot} (within calibration rounding 1e-3)"
    )


@pytest.mark.parametrize(
    "q_nero, q_airbot",
    CALIBRATION_PAIRS,
    ids=[f"pose-{idx + 1}" for idx in range(len(CALIBRATION_PAIRS))],
)
def test_command_fix_inverts_publish_fix_on_calibration_pairs(
    q_nero: tuple[float, float, float, float],
    q_airbot: tuple[float, float, float, float],
) -> None:
    recovered = apply_command_pose_fix(_wrap_pose(q_airbot))
    assert _quat_close_xyzw(tuple(recovered[3:7]), q_nero, tol=1e-3)


def test_publish_fix_rotates_translation_by_q_base_180_yaw() -> None:
    # On this rig q_base = R_z(180 deg), so the published xyz must be
    # (x, y, z) -> (-x, -y, z). This is the bug fix: with orientation-
    # only remapping the published x/y stayed in the Nero base frame
    # and ended up mirrored in the AIRBOT-aligned task frame.
    pose = [0.123, -0.456, 0.789, 0.0, 0.0, 0.0, 1.0]
    out = apply_publish_pose_fix(pose)
    assert math.isclose(out[0], -0.123, abs_tol=1e-12)
    assert math.isclose(out[1], 0.456, abs_tol=1e-12)
    assert math.isclose(out[2], 0.789, abs_tol=1e-12)


def test_command_fix_rotates_translation_by_q_base_inverse() -> None:
    # R_z(180 deg) is its own inverse, so the command direction also
    # flips x and y. Together with the publish direction, an ingest
    # then re-emit must round-trip translation exactly.
    pose = [0.123, -0.456, 0.789, 0.0, 0.0, 0.0, 1.0]
    out = apply_command_pose_fix(pose)
    assert math.isclose(out[0], -0.123, abs_tol=1e-12)
    assert math.isclose(out[1], 0.456, abs_tol=1e-12)
    assert math.isclose(out[2], 0.789, abs_tol=1e-12)


def test_publish_then_command_round_trip_recovers_input_to_machine_precision() -> None:
    # Use a non-trivial, asymmetric quaternion + translation so any
    # sign or product mistake surfaces in the round-trip rather than
    # being masked by the calibration fixture's symmetric values.
    q = (0.182574, 0.365148, 0.547723, 0.730297)
    norm = math.sqrt(sum(c * c for c in q))
    q_unit = tuple(c / norm for c in q)
    pose = [0.21, -0.34, 0.55, q_unit[0], q_unit[1], q_unit[2], q_unit[3]]

    intermediate = apply_publish_pose_fix(pose)
    recovered = apply_command_pose_fix(intermediate)

    assert _quat_close_xyzw(tuple(recovered[3:7]), q_unit, tol=1e-12), (
        f"round-trip orientation drifted: {recovered[3:7]} vs {q_unit}"
    )
    for axis_idx, axis_name in enumerate(("x", "y", "z")):
        assert math.isclose(recovered[axis_idx], pose[axis_idx], abs_tol=1e-12), (
            f"round-trip translation drifted on {axis_name}: "
            f"{recovered[axis_idx]} vs {pose[axis_idx]}"
        )


def test_publish_fix_output_is_unit_norm() -> None:
    for q_nero, _ in CALIBRATION_PAIRS:
        out = apply_publish_pose_fix(_wrap_pose(q_nero))
        norm = math.sqrt(sum(c * c for c in out[3:7]))
        assert abs(norm - 1.0) < 1e-9


def test_command_fix_output_is_unit_norm() -> None:
    for _, q_airbot in CALIBRATION_PAIRS:
        out = apply_command_pose_fix(_wrap_pose(q_airbot))
        norm = math.sqrt(sum(c * c for c in out[3:7]))
        assert abs(norm - 1.0) < 1e-9


def test_constants_are_unit_quaternions() -> None:
    for q in (Q_BASE_FIX_XYZW, Q_TCP_FIX_XYZW):
        norm = math.sqrt(sum(c * c for c in q))
        assert abs(norm - 1.0) < 1e-12


def test_pose7_length_validation() -> None:
    with pytest.raises(ValueError):
        apply_publish_pose_fix([0.0] * 6)
    with pytest.raises(ValueError):
        apply_command_pose_fix([0.0] * 8)

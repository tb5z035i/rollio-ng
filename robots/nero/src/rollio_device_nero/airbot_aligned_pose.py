"""AIRBOT-aligned `EndEffectorPose` adapter for the Nero driver.

Both the AGX Nero driver (this crate) and the AIRBOT Play driver
(`robots/airbot_play_rust`) publish `Pose7` over iceoryx2 with the wire
format `[x, y, z, qx, qy, qz, qw]` -- scalar-last (xyzw). The wire
ordering itself is consistent end-to-end on both sides:

  * AIRBOT analytical FK: `play_analytical.rs::matrix_to_pose` writes
    `[quat.i, quat.j, quat.k, quat.w]`.
  * AIRBOT Pinocchio FFI: `ffi/pinocchio_shim.cpp::pose_to_rust_vec`
    writes `quat.x, quat.y, quat.z, quat.w`.
  * Nero FK: `gravity.py::end_effector_pose7` writes
    `quat.x, quat.y, quat.z, quat.w`.
  * Nero IK ingest: `ik.py::_pose7_to_se3` reads `qx, qy, qz, qw` and
    rebuilds `pin.Quaternion(w, x, y, z)` correctly.

But each driver builds the pose in its own physical *frame*:

  * The Nero arm on this rig is physically *mounted* with its base
    rotated 180 degrees about the world vertical axis relative to the
    AIRBOT arm. Both drivers report the EE pose in the arm's own base
    frame, so the published positions disagree by `R_z(180 deg)`
    (i.e. `(x_n, y_n, z_n) -> (-x_n, -y_n, z_n)` to land in the
    AIRBOT base frame). This is treated as a fixed `q_base` rotation.
  * Each driver also picks a different TCP / tool frame at the
    flange. Nero ends in a Pinocchio operational frame whose flange
    RPY is `(-pi/2, 0, -pi/2)` (see `TOOL_FLANGE_RPY`); AIRBOT
    post-multiplies its analytical chain by `end_convert` whose RPY
    is `(pi/2, -pi/2, 0)`. This is treated as a fixed `q_tcp` rotation
    that affects the orientation only -- TCP origins are assumed
    coincident here (no translation in `T_tcp`).

Modeling each piece as a 4x4 rigid transform gives the SE(3) relation

    T_airbot = T_base @ T_nero_native @ T_tcp^{-1}

with `T_base = (R_base, 0)`, `T_tcp = (R_tcp, 0)` (both pure
rotations). Working that out for the published pose components:

    R_airbot = R_base @ R_nero @ R_tcp^T
    t_airbot = R_base @ t_nero

The orientation half was confirmed against six jig-matched physical
poses recorded on hardware (see `tests/test_airbot_aligned_pose.py`)
and locked in by `Q_BASE_FIX_XYZW` and `Q_TCP_FIX_XYZW`. The
translation half follows from the same `q_base` -- it was added
once the orientation fix exposed the leftover x/y sign disagreement
between the two reports.

If a future calibration shows that the TCP origins also differ
between the two arms, extend `T_tcp` to a full SE(3) by adding a
constant TCP-frame translation.
"""

from __future__ import annotations

import math
from collections.abc import Sequence

# 1/sqrt(2) to full f64 precision. Using `math.sqrt(2.0)` rather than the
# truncated `0.707` literal keeps `apply_command(apply_publish(p)) == p`
# accurate to ~1e-16 instead of ~1e-3.
_INV_SQRT_2 = 1.0 / math.sqrt(2.0)

#: Base / world rotation. The Nero arm is *physically* mounted with
#: its base rotated 180 degrees about the world vertical axis relative
#: to the AIRBOT arm; this constant maps a vector from Nero's base
#: frame into the AIRBOT base frame. xyzw form of `R_z(180 deg)`.
Q_BASE_FIX_XYZW: tuple[float, float, float, float] = (0.0, 0.0, 1.0, 0.0)

#: TCP / tool rotation. AIRBOT's TCP frame is a 180-degree rotation
#: about the axis (1, 0, 1) / sqrt(2) (expressed in Nero's TCP frame)
#: away from Nero's TCP frame. xyzw form. TCP origin is assumed
#: coincident; only the axis triad differs.
Q_TCP_FIX_XYZW: tuple[float, float, float, float] = (
    _INV_SQRT_2,
    0.0,
    _INV_SQRT_2,
    0.0,
)


def _quat_mul_xyzw(
    a: Sequence[float],
    b: Sequence[float],
) -> tuple[float, float, float, float]:
    """Hamilton product `a @ b` for scalar-last (xyzw) unit quaternions."""
    ax, ay, az, aw = a[0], a[1], a[2], a[3]
    bx, by, bz, bw = b[0], b[1], b[2], b[3]
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def _quat_inv_xyzw(q: Sequence[float]) -> tuple[float, float, float, float]:
    """Inverse (conjugate) of a unit xyzw quaternion."""
    return (-q[0], -q[1], -q[2], q[3])


def _rotate_vec_by_quat_xyzw(
    q: Sequence[float],
    v: Sequence[float],
) -> tuple[float, float, float]:
    """Rotate `v` (xyz) by unit quaternion `q` (xyzw): `v' = q @ v @ q^{-1}`.

    Implemented via the explicit cross-product form

        v' = v + 2 * cross(qv, cross(qv, v) + qw * v)

    which matches `q_b @ (0, vx, vy, vz) @ q_b^{-1}` exactly while
    avoiding two full Hamilton products per call.
    """
    qx, qy, qz, qw = q[0], q[1], q[2], q[3]
    vx, vy, vz = v[0], v[1], v[2]

    # u = cross(qv, v) + qw * v
    ux = qy * vz - qz * vy + qw * vx
    uy = qz * vx - qx * vz + qw * vy
    uz = qx * vy - qy * vx + qw * vz

    # v' = v + 2 * cross(qv, u)
    return (
        vx + 2.0 * (qy * uz - qz * uy),
        vy + 2.0 * (qz * ux - qx * uz),
        vz + 2.0 * (qx * uy - qy * ux),
    )


# Pre-compute the constant inverses so the hot path stays at two
# multiply-adds and zero allocations of intermediate constants.
_Q_BASE_FIX_INV_XYZW: tuple[float, float, float, float] = _quat_inv_xyzw(Q_BASE_FIX_XYZW)
_Q_TCP_FIX_INV_XYZW: tuple[float, float, float, float] = _quat_inv_xyzw(Q_TCP_FIX_XYZW)


def apply_publish_pose_fix(pose7: Sequence[float]) -> list[float]:
    """Convert a native-Nero `Pose7` to the AIRBOT-aligned reporting frame.

    Both the orientation and the position are remapped:

        q_out = Q_BASE_FIX_XYZW @ q_native @ Q_TCP_FIX_XYZW^{-1}
        t_out = Q_BASE_FIX_XYZW (t_native)        # rotate by base only

    Returns a new `list[float]` of length 7 with the same
    `[x, y, z, qx, qy, qz, qw]` layout as the input, ready to be handed
    to `Pose7.from_values`.
    """
    if len(pose7) != 7:
        raise ValueError(f"Pose7 must have 7 values, got {len(pose7)}")

    t_native = (float(pose7[0]), float(pose7[1]), float(pose7[2]))
    tx, ty, tz = _rotate_vec_by_quat_xyzw(Q_BASE_FIX_XYZW, t_native)

    q_native = (
        float(pose7[3]),
        float(pose7[4]),
        float(pose7[5]),
        float(pose7[6]),
    )
    qx, qy, qz, qw = _quat_mul_xyzw(
        _quat_mul_xyzw(Q_BASE_FIX_XYZW, q_native),
        _Q_TCP_FIX_INV_XYZW,
    )
    return [tx, ty, tz, qx, qy, qz, qw]


def apply_command_pose_fix(pose7: Sequence[float]) -> list[float]:
    """Convert an AIRBOT-aligned `Pose7` command back to Nero's native frame.

    The exact inverse of `apply_publish_pose_fix`:

        q_out = Q_BASE_FIX_XYZW^{-1} @ q_aligned @ Q_TCP_FIX_XYZW
        t_out = Q_BASE_FIX_XYZW^{-1} (t_aligned)
    """
    if len(pose7) != 7:
        raise ValueError(f"Pose7 must have 7 values, got {len(pose7)}")

    t_aligned = (float(pose7[0]), float(pose7[1]), float(pose7[2]))
    tx, ty, tz = _rotate_vec_by_quat_xyzw(_Q_BASE_FIX_INV_XYZW, t_aligned)

    q_aligned = (
        float(pose7[3]),
        float(pose7[4]),
        float(pose7[5]),
        float(pose7[6]),
    )
    qx, qy, qz, qw = _quat_mul_xyzw(
        _quat_mul_xyzw(_Q_BASE_FIX_INV_XYZW, q_aligned),
        Q_TCP_FIX_XYZW,
    )
    return [tx, ty, tz, qx, qy, qz, qw]


__all__ = [
    "Q_BASE_FIX_XYZW",
    "Q_TCP_FIX_XYZW",
    "apply_command_pose_fix",
    "apply_publish_pose_fix",
]

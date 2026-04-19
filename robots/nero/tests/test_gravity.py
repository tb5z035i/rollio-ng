"""Pinocchio gravity-model sanity checks for `rollio_device_nero.gravity`."""

from __future__ import annotations

import pytest

pin = pytest.importorskip("pinocchio")
np = pytest.importorskip("numpy")

from rollio_device_nero.gravity import GRIPPER_TCP_DEPTH_M, NeroModel  # noqa: E402

# Canonical g(q=0) values produced by `external/reference/nero-demo/gravity_compensation.py --dry-run`
# on this same URDF. Captured 2026-04-19 -- if the URDF asset changes, regenerate.
_G_BARE = [
    -9.61597462e-23,
    9.13160076e-03,
    2.95688642e-09,
    1.35982690e-02,
    1.10660934e-09,
    -3.32179269e-03,
    9.81000090e-10,
]
_G_WITH_GRIPPER = [
    -1.29246971e-22,
    8.32071456e-03,
    2.88046127e-09,
    1.27874720e-02,
    1.06145207e-09,
    -3.67792988e-03,
    -8.10665016e-04,
]
_TOL = 1e-6


def test_bare_arm_gravity_at_zero_matches_reference() -> None:
    nero = NeroModel(with_gripper=False)
    assert nero.nq == 7 and nero.nv == 7
    g = nero.gravity_torques(np.zeros(7))
    assert g.shape == (7,)
    assert np.allclose(g, _G_BARE, atol=_TOL)


def test_with_gripper_gravity_at_zero_matches_reference() -> None:
    nero = NeroModel(with_gripper=True)
    g = nero.gravity_torques(np.zeros(7))
    assert np.allclose(g, _G_WITH_GRIPPER, atol=_TOL)
    # The four-body sub-tree should sum to ~0.55 kg per the AGX gripper xacro.
    assert nero.gripper_summary is not None
    assert abs(nero.gripper_summary["total_mass"] - 0.5477) < 1e-3


def test_gravity_torques_clipped_respects_tau_max() -> None:
    nero = NeroModel(with_gripper=True)
    # Push the model into a configuration with a known large gravity torque
    # (joint 4 hangs out horizontally) and confirm clipping caps at 18 N*m.
    q = np.zeros(7)
    q[1] = -np.pi / 2  # tip joint 2 forward
    g = nero.gravity_torques(q)
    g_clipped = nero.gravity_torques_clipped(q)
    assert np.all(np.abs(g_clipped) <= np.array([24.0, 24.0, 18.0, 18.0, 8.0, 8.0, 8.0]) + 1e-9)
    # And clipping is a no-op wherever |g| was already within the cap.
    bound = np.array([24.0, 24.0, 18.0, 18.0, 8.0, 8.0, 8.0])
    safe = np.abs(g) <= bound
    assert np.allclose(g_clipped[safe], g[safe])


def test_end_effector_pose_round_trips_quaternion() -> None:
    nero = NeroModel(with_gripper=False)
    pose = nero.end_effector_pose7(np.zeros(7))
    assert len(pose) == 7
    qx, qy, qz, qw = pose[3:]
    norm = np.sqrt(qx * qx + qy * qy + qz * qz + qw * qw)
    assert abs(norm - 1.0) < 1e-9


def test_gripper_tcp_is_offset_from_bare_flange_by_constant_depth() -> None:
    """When the gripper channel is enabled, FK / IK must publish the TCP
    (gripper midpoint at the fingertip plane), not the bare flange.

    The TCP sits exactly `GRIPPER_TCP_DEPTH_M` along the gripper-flange
    z-axis -- the manually-measured length of the AGX gripper assembly.
    """
    nero_bare = NeroModel(with_gripper=False)
    nero_grip = NeroModel(with_gripper=True)

    bare = np.asarray(nero_bare.end_effector_pose7(np.zeros(7))[:3])
    grip = np.asarray(nero_grip.end_effector_pose7(np.zeros(7))[:3])

    delta = grip - bare
    assert abs(np.linalg.norm(delta) - GRIPPER_TCP_DEPTH_M) < 1e-9

    # The TCP shift is along the gripper-z axis. At q=0 the gripper-z is
    # collinear with base-z (the arm points straight up), so the entire
    # offset shows up in the world-z component.
    assert abs(delta[0]) < 1e-6
    assert abs(delta[1]) < 1e-6
    assert abs(delta[2] - GRIPPER_TCP_DEPTH_M) < 1e-9

    # Quaternion (orientation) must be the same in both cases since the
    # default TCP only translates along the flange's z, no rotation.
    pose_bare_quat = nero_bare.end_effector_pose7(np.zeros(7))[3:]
    pose_grip_quat = nero_grip.end_effector_pose7(np.zeros(7))[3:]
    for a, b in zip(pose_bare_quat, pose_grip_quat, strict=False):
        assert abs(a - b) < 1e-6


def test_end_effector_translation_changes_when_only_joint7_rotates() -> None:
    """Regression: the reported tool tip translation MUST change when only
    joint7 rotates. The earlier implementation returned the joint7 axis
    frame, whose origin is on the rotation axis -- so dragging only the
    wrist left translation invariant. The off-axis tip frame fixes it
    (true for both bare-flange and gripper-TCP cases).
    """
    nero = NeroModel(with_gripper=False)
    pose_zero = nero.end_effector_pose7(np.zeros(7))

    q_pos = np.zeros(7)
    q_pos[6] = 1.0
    pose_pos = nero.end_effector_pose7(q_pos)

    q_neg = np.zeros(7)
    q_neg[6] = -1.0
    pose_neg = nero.end_effector_pose7(q_neg)

    translation_zero = np.asarray(pose_zero[:3])
    translation_pos = np.asarray(pose_pos[:3])
    translation_neg = np.asarray(pose_neg[:3])

    # Flange should sweep through ~ 0.032 * sin(1 rad) = ~0.027 m off-axis.
    assert np.linalg.norm(translation_pos - translation_zero) > 0.02
    assert np.linalg.norm(translation_neg - translation_zero) > 0.02

    # Symmetric: ±θ rotation produces opposite-sign offsets in the off-axis
    # direction (joint7 rotates about its z-axis, the +0.032 m flange offset
    # along x sweeps to ∓ in base coords).
    delta_pos = translation_pos - translation_zero
    delta_neg = translation_neg - translation_zero
    assert delta_pos @ delta_neg < 0  # vectors point opposite ways

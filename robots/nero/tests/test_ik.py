"""CLIK convergence tests for `rollio_device_nero.ik`."""

from __future__ import annotations

import pytest

pin = pytest.importorskip("pinocchio")
np = pytest.importorskip("numpy")

from rollio_device_nero.gravity import NeroModel  # noqa: E402
from rollio_device_nero.ik import solve  # noqa: E402


def _pose_distance(pin_module, a: list[float], b: list[float]) -> tuple[float, float]:
    """Return (translation_distance_m, rotation_angle_rad) between two Pose7."""
    p_a = np.asarray(a[0:3])
    p_b = np.asarray(b[0:3])
    quat_a = pin_module.Quaternion(float(a[6]), float(a[3]), float(a[4]), float(a[5]))
    quat_b = pin_module.Quaternion(float(b[6]), float(b[3]), float(b[4]), float(b[5]))
    quat_a.normalize()
    quat_b.normalize()
    rel = quat_a.toRotationMatrix().T @ quat_b.toRotationMatrix()
    cos_angle = (np.trace(rel) - 1.0) * 0.5
    cos_angle = max(-1.0, min(1.0, float(cos_angle)))
    return float(np.linalg.norm(p_a - p_b)), float(np.arccos(cos_angle))


def test_solve_recovers_q_zero_when_target_equals_fk_zero() -> None:
    nero = NeroModel(with_gripper=False)
    target = nero.end_effector_pose7(np.zeros(7))

    q, converged, err = solve(nero, target, q0=np.zeros(7))

    assert converged
    assert err < 1e-4
    # Recovered q should be close to zero (arm is already at the target pose).
    assert np.linalg.norm(q) < 1e-3


def test_solve_converges_to_small_displacement() -> None:
    nero = NeroModel(with_gripper=False)
    pose0 = nero.end_effector_pose7(np.zeros(7))
    target = list(pose0)
    target[0] += 0.05  # +5 cm in x
    target[2] -= 0.03  # -3 cm in z

    q, converged, err = solve(nero, target, q0=np.zeros(7))

    assert converged, f"IK did not converge, final err={err}"
    pose_final = nero.end_effector_pose7(q)
    trans_err, rot_err = _pose_distance(pin, target, pose_final)
    assert trans_err < 1e-3  # < 1 mm
    assert rot_err < 1e-3  # < 1 mrad


_ROUND_TRIP_CONFIGS: list[tuple[str, list[float]]] = [
    ("zero", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    # Mid-workspace home pose (slight elbow bend).
    ("home_bent", [0.0, -0.5, 0.0, 0.7, 0.0, 0.3, 0.0]),
    # Wrist twist: only joint7 nonzero. Catches the joint7-axis-vs-flange
    # frame bug -- if FK and IK disagreed on the tip frame, IK would not be
    # able to reproduce this pose.
    ("wrist_only", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
    ("wrist_negative", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.2]),
    # Asymmetric pose touching most joints.
    ("asymmetric", [0.5, -0.3, 0.4, 0.6, -0.2, 0.4, 0.8]),
    # Near-limit pose (within URDF bounds).
    ("near_limit", [2.0, 1.2, -2.0, 1.8, -2.0, 0.6, 1.4]),
]


@pytest.mark.parametrize("with_gripper", [False, True], ids=["bare_flange", "gripper_tcp"])
@pytest.mark.parametrize(
    "name,q_seed", _ROUND_TRIP_CONFIGS, ids=[c[0] for c in _ROUND_TRIP_CONFIGS]
)
def test_fk_ik_round_trip_recovers_pose(name: str, q_seed: list[float], with_gripper: bool) -> None:
    """For any reachable q, IK(FK(q)) must yield some q' with FK(q') ≈ FK(q).

    Joint-space recovery is NOT required (the Nero is 7-DOF redundant: many
    q give the same pose). What MUST hold is the pose-space round-trip --
    if it ever doesn't, FK and IK disagree on the tip frame or the
    Jacobian / log6 pairing is broken.

    Tested for both tip-frame defaults: the bare flange (gripper not
    mounted) AND the gripper TCP (`+GRIPPER_TCP_DEPTH_M` along flange-z).
    """
    nero = NeroModel(with_gripper=with_gripper)
    q = np.asarray(q_seed, dtype=float)
    target = nero.end_effector_pose7(q)

    # Warm-start from a small perturbation of `q` so we don't trivially
    # short-circuit the loop on the first iteration.
    rng = np.random.default_rng(seed=(hash((name, with_gripper))) & 0xFFFF)
    q0 = q + rng.uniform(-0.05, 0.05, size=q.shape)

    q_solved, converged, err = solve(nero, target, q0=q0)
    assert converged, f"[{name}/{with_gripper=}] IK did not converge, final err={err}"

    pose_solved = nero.end_effector_pose7(q_solved)
    trans_err, rot_err = _pose_distance(pin, target, pose_solved)
    assert trans_err < 1e-3, (
        f"[{name}/{with_gripper=}] translation round-trip error {trans_err * 1000:.3f} mm"
    )
    assert rot_err < 1e-3, (
        f"[{name}/{with_gripper=}] rotation round-trip error {rot_err * 1000:.3f} mrad"
    )


@pytest.mark.parametrize("with_gripper", [False, True], ids=["bare_flange", "gripper_tcp"])
def test_fk_ik_round_trip_with_random_configurations(with_gripper: bool) -> None:
    """Sweep 30 random joint configurations sampled inside the URDF limits
    and assert pose-space round-trip on every one. Runs against both the
    bare-flange and the gripper-TCP tip frame defaults."""
    from rollio_device_nero.query import (
        ARM_JOINT_POSITION_MAX,
        ARM_JOINT_POSITION_MIN,
    )

    nero = NeroModel(with_gripper=with_gripper)
    lb = np.asarray(ARM_JOINT_POSITION_MIN)
    ub = np.asarray(ARM_JOINT_POSITION_MAX)
    # Shrink toward the interior so we don't sit on a singularity at the
    # wrist limits where the damped Jacobian struggles.
    margin = 0.1
    lb_inner = lb + margin
    ub_inner = ub - margin

    rng = np.random.default_rng(seed=20260419)
    failures: list[str] = []
    for trial in range(30):
        q = rng.uniform(lb_inner, ub_inner)
        target = nero.end_effector_pose7(q)
        # Warm-start from a moderate perturbation -- realistic for teleop
        # where consecutive targets are close together.
        q0 = q + rng.uniform(-0.1, 0.1, size=q.shape)
        q0 = np.clip(q0, lb, ub)
        q_solved, converged, err = solve(nero, target, q0=q0)
        if not converged:
            failures.append(f"trial {trial}: IK diverged, err={err}")
            continue
        pose_solved = nero.end_effector_pose7(q_solved)
        trans_err, rot_err = _pose_distance(pin, target, pose_solved)
        if trans_err >= 1e-3 or rot_err >= 1e-3:
            failures.append(
                f"trial {trial}: trans_err={trans_err * 1000:.3f}mm, "
                f"rot_err={rot_err * 1000:.3f}mrad"
            )

    assert not failures, "FK/IK round-trip failed on:\n  " + "\n  ".join(failures)


def test_fk_then_ik_then_fk_is_idempotent() -> None:
    """A second IK from the previous q_solved must produce the same pose.

    Catches drift bugs where the IK solution depends on the warm-start in
    a way that creates a moving fixed point.
    """
    nero = NeroModel(with_gripper=False)
    q = np.asarray([0.4, -0.3, 0.5, 0.7, -0.2, 0.4, 0.6])
    target = nero.end_effector_pose7(q)

    # First round trip.
    q1, conv1, _ = solve(nero, target, q0=q)
    assert conv1
    pose1 = nero.end_effector_pose7(q1)

    # Second round trip warm-started from q1.
    q2, conv2, _ = solve(nero, pose1, q0=q1)
    assert conv2
    pose2 = nero.end_effector_pose7(q2)

    trans_err, rot_err = _pose_distance(pin, pose1, pose2)
    assert trans_err < 1e-6
    assert rot_err < 1e-6


def test_solve_clips_to_joint_limits() -> None:
    """Even when the requested motion would exceed limits, the solution
    must stay inside them (the controller still issues kp/kd-bounded
    commands so the operator gets a partial response)."""
    nero = NeroModel(with_gripper=False)
    # Aggressive Cartesian target far outside the workspace.
    pose0 = nero.end_effector_pose7(np.zeros(7))
    target = list(pose0)
    target[0] += 5.0  # ridiculous reach

    q, _converged, _err = solve(nero, target, q0=np.zeros(7))

    from rollio_device_nero.query import (
        ARM_JOINT_POSITION_MAX,
        ARM_JOINT_POSITION_MIN,
    )

    lb = np.asarray(ARM_JOINT_POSITION_MIN)
    ub = np.asarray(ARM_JOINT_POSITION_MAX)
    assert np.all(q >= lb - 1e-9)
    assert np.all(q <= ub + 1e-9)

"""Damped-pseudo-inverse Cartesian IK for the AGX Nero arm.

The Nero arm has 7-DOF (i.e. one redundant DOF), so we cannot use a
closed-form analytical inverse like airbot_play_rust does. Instead we run
a closed-loop IK (CLIK) iteration in the link7 LOCAL frame:

    while iter < max_iter:
        oMi    = FK(q)
        err6   = pin.log6(oMi.actInv(target))      # 6-vec twist from oMi to target
        if ||err6|| < tol: break
        J      = pin.computeJointJacobian(model, data, q, link7_id)   # 6 x 7 in LOCAL
        dq     = solve((J.T @ J + lambda^2 * I) , J.T @ err6)         # damped pseudo-inverse
        q     += step * dq

This converges in a couple of iterations when warm-started from `q_meas`
under teleop where consecutive Cartesian targets are close together.
"""

from __future__ import annotations

import numpy as np

from .gravity import NeroModel
from .query import ARM_JOINT_POSITION_MAX, ARM_JOINT_POSITION_MIN

_DEFAULT_MAX_ITER: int = 50
_DEFAULT_DAMPING: float = 1e-2
_DEFAULT_TOL: float = 1e-4  # ~0.1 mm and ~0.1 mrad for translation/rotation in pin.log6
_DEFAULT_STEP: float = 1.0
# Default null-space regularization weight. With Nero's 7-DOF (one
# redundant joint), the damped pseudo-inverse warm-started from the
# previous IK output is free to drift along the null space tick-to-tick:
# the elbow can swing through tens of degrees while the EE barely moves,
# producing the visible "stutter" / 卡一卡 in cartesian teleop. Adding a
# small regularizer that pulls each iteration toward `q_anchor` (passed
# in as the live `q_meas` from the runtime) collapses that null-space
# freedom onto a single stable configuration without measurably
# degrading cartesian tracking accuracy. mu^2 = 2.5e-3 sits one order
# of magnitude above damping^2 = 1e-4, so the anchor wins in the null
# space (where J^T err = 0) but stays sub-dominant where cartesian
# error is non-zero.
_DEFAULT_ANCHOR_WEIGHT: float = 0.005

_JOINT_LB: np.ndarray = np.asarray(ARM_JOINT_POSITION_MIN, dtype=float)
_JOINT_UB: np.ndarray = np.asarray(ARM_JOINT_POSITION_MAX, dtype=float)


def _pose7_to_se3(pin: object, pose: list[float] | np.ndarray) -> object:
    values = np.asarray(pose, dtype=float).reshape(7)
    translation = values[0:3]
    qx, qy, qz, qw = values[3], values[4], values[5], values[6]
    quat = pin.Quaternion(float(qw), float(qx), float(qy), float(qz))  # type: ignore[attr-defined]
    quat.normalize()
    return pin.SE3(quat.toRotationMatrix(), translation)  # type: ignore[attr-defined]


def solve(
    nero: NeroModel,
    target_pose7: list[float] | np.ndarray,
    *,
    q0: np.ndarray | None = None,
    q_anchor: np.ndarray | None = None,
    anchor_weight: float = _DEFAULT_ANCHOR_WEIGHT,
    max_iter: int = _DEFAULT_MAX_ITER,
    damping: float = _DEFAULT_DAMPING,
    tol: float = _DEFAULT_TOL,
    step: float = _DEFAULT_STEP,
) -> tuple[np.ndarray, bool, float]:
    """Solve `IK(target) ≈ q` warm-started from `q0`.

    Optional null-space anchoring: when `q_anchor` is provided and
    `anchor_weight > 0`, each damped-LS iteration also penalises
    `||q + dq - q_anchor||^2` so the redundant DOF is pulled toward the
    anchor configuration. The augmented normal equation is

        (J^T J + λ^2 I + μ^2 I) dq = J^T e + μ^2 (q_anchor - q)

    where `μ = anchor_weight`. With Nero's 7-DOF arm (one redundant
    joint) this collapses null-space freedom onto a single stable
    configuration tick-to-tick, eliminating the elbow-swing drift the
    bare damped pseudo-inverse exhibited under teleop warm-starting.

    Returns `(q, converged, final_err_norm)`. `converged` is True iff the
    final 6D error norm drops below `tol`. The returned `q` is always
    clipped to the URDF joint limits, even when the iteration does not
    converge -- the runtime will still send it (with caller-imposed kp/kd)
    so the operator can see a partial response instead of nothing.
    """
    pin = nero._pin
    target = _pose7_to_se3(pin, target_pose7)

    q = np.zeros(nero.nq, dtype=float) if q0 is None else np.array(q0, dtype=float, copy=True)
    if q.shape != (nero.nq,):
        raise ValueError(f"q0 must have shape ({nero.nq},), got {q.shape}")

    use_anchor = q_anchor is not None and anchor_weight > 0.0
    if use_anchor:
        q_anchor_arr = np.asarray(q_anchor, dtype=float).reshape(nero.nv)
        mu_sq = float(anchor_weight) * float(anchor_weight)
    else:
        q_anchor_arr = None
        mu_sq = 0.0

    err_norm = float("inf")
    for _ in range(max_iter):
        oMi = nero.forward_kinematics(q)
        # `actInv(target)` gives target expressed in the link7 local frame;
        # `log6` returns the 6-twist that maps current → target in that frame.
        err = np.asarray(pin.log6(oMi.actInv(target)).vector, dtype=float)
        err_norm = float(np.linalg.norm(err))
        if err_norm < tol and not use_anchor:
            # Without anchor, converged on cartesian => done. With anchor we
            # keep iterating so the null-space term can pull q toward the
            # anchor; the anchor bias is benign once cartesian is satisfied
            # because (q_anchor - q) only moves through the null space.
            return _clip_to_limits(q), True, err_norm

        jac = nero.frame_jacobian(q)  # 6 x nv in link7's LOCAL frame
        # Damped least-squares with optional null-space anchor:
        #   dq = (J^T J + λ²I + μ²I)^-1 (J^T e + μ²(q_anchor - q))
        m_diag = damping * damping + mu_sq
        jt_j = jac.T @ jac + m_diag * np.eye(nero.nv)
        rhs = jac.T @ err
        if use_anchor:
            rhs = rhs + mu_sq * (q_anchor_arr - q)
        try:
            dq = np.linalg.solve(jt_j, rhs)
        except np.linalg.LinAlgError:
            return _clip_to_limits(q), False, err_norm
        q = q + step * dq
        q = _clip_to_limits(q)

        if use_anchor and err_norm < tol:
            # With anchor, exit once both cartesian is satisfied AND the
            # anchor pull is small (||dq|| would shrink as q approaches
            # the null-space-projected anchor). One step past convergence
            # is enough to bleed off any remaining null-space slack.
            return _clip_to_limits(q), True, err_norm

    return q, False, err_norm


def _clip_to_limits(q: np.ndarray) -> np.ndarray:
    return np.minimum(np.maximum(q, _JOINT_LB), _JOINT_UB)


__all__ = ["solve"]

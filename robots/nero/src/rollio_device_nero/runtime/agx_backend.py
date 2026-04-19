"""Concrete `ArmBackend` / `GripperBackend` adapters over `pyAgxArm`.

Imports the SDK lazily so the package stays importable on hosts where
`pyAgxArm` is not installed (e.g. CI containers running config / IPC tests).
"""

from __future__ import annotations

from contextlib import suppress
from typing import Any

import numpy as np

from .. import ARM_DOF


def _load_pyagxarm() -> Any:
    try:
        import pyAgxArm  # noqa: PLC0415
    except Exception as exc:  # pragma: no cover - host-dependent
        raise RuntimeError(
            "pyAgxArm is required for `rollio-device-nero run`; install the "
            "third_party/pyAgxArm submodule (uv sync inside robots/nero)."
        ) from exc
    return pyAgxArm


def create_robot(interface: str) -> Any:
    """Create + connect a Nero CAN driver bound to `interface`.

    The driver runs its own background reader thread (started by `connect()`),
    so subsequent `get_*` calls are non-blocking.
    """
    pyAgxArm = _load_pyagxarm()
    cfg = pyAgxArm.create_agx_arm_config(
        robot=pyAgxArm.ArmModel.NERO,
        firmeware_version=pyAgxArm.NeroFW.DEFAULT,
        channel=interface,
    )
    robot = pyAgxArm.AgxArmFactory.create_arm(cfg)
    robot.connect()
    return robot


def enable_robot(robot: Any, *, max_retries: int = 200, retry_sleep_s: float = 0.01) -> bool:
    """Run the canonical enable handshake from `external/reference/nero-demo/test.py`.

    Returns True iff `enable()` reports all motors enabled within the
    retry budget (default ~2 s). The runtime calls this once before
    starting the per-channel loops; if it returns False we still proceed
    so that the operator can use Disabled-mode hold to bring the arm to
    a safe pose.

    After a successful enable, the arm is locked into MIT motion mode and
    the driver's `auto_set_motion_mode` is disabled so subsequent
    `move_mit(...)` calls don't re-send a redundant mode-set CAN frame
    every tick. With our 7-DOF NERO at 250 Hz, the default behaviour cost
    ~half the CAN bus to redundant mode-set frames (7 mode-set + 7
    mit-cmd per tick × 250 = 3500 frames/s ≈ 469 ms/s on a 1 Mbps bus),
    starving feedback frames and producing visible follower stutter.
    """
    import time as _time

    enabled = False
    for _ in range(max_retries):
        if robot.enable():
            enabled = True
            break
        with suppress(Exception):
            robot.set_normal_mode()
        _time.sleep(retry_sleep_s)

    # Whether or not enable() ultimately reported success, lock in MIT
    # mode and stop the driver from re-sending it on every `move_mit`.
    # `set_motion_mode` is idempotent CAN-traffic-wise (one frame), and
    # `set_auto_set_motion_mode_enabled(False)` is a pure local flag.
    with suppress(Exception):
        robot.set_motion_mode("mit")
    with suppress(Exception):
        robot.set_auto_set_motion_mode_enabled(False)
    return enabled


def init_gripper(robot: Any) -> Any:
    """Wire up the AGX gripper effector and return its driver."""
    return robot.init_effector("agx_gripper")


# ---------------------------------------------------------------------------
# ArmBackend impl
# ---------------------------------------------------------------------------


class AgxArmBackend:
    """Wraps a `pyAgxArm` Nero driver as the runtime's `ArmBackend`."""

    def __init__(self, robot: Any) -> None:
        self._robot = robot

    def get_joint_angles_array(self) -> np.ndarray | None:
        ja = self._robot.get_joint_angles()
        if ja is None:
            return None
        msg = ja.msg
        if not isinstance(msg, list) or len(msg) < ARM_DOF:
            return None
        return np.asarray(msg[:ARM_DOF], dtype=float)

    def get_joint_velocities_array(self) -> np.ndarray | None:
        out = np.zeros(ARM_DOF)
        any_seen = False
        for i in range(ARM_DOF):
            ms = self._robot.get_motor_states(i + 1)
            if ms is None:
                continue
            out[i] = float(ms.msg.velocity)
            any_seen = True
        return out if any_seen else None

    def get_joint_efforts_array(self) -> np.ndarray | None:
        out = np.zeros(ARM_DOF)
        any_seen = False
        for i in range(ARM_DOF):
            ms = self._robot.get_motor_states(i + 1)
            if ms is None:
                continue
            out[i] = float(ms.msg.torque)
            any_seen = True
        return out if any_seen else None

    def move_mit(
        self,
        joint_index: int,
        p_des: float,
        v_des: float,
        kp: float,
        kd: float,
        t_ff: float,
    ) -> None:
        self._robot.move_mit(
            joint_index=joint_index,
            p_des=p_des,
            v_des=v_des,
            kp=kp,
            kd=kd,
            t_ff=t_ff,
        )


# ---------------------------------------------------------------------------
# GripperBackend impl
# ---------------------------------------------------------------------------


class AgxGripperBackend:
    """Wraps a `pyAgxArm` AGX gripper effector as the runtime's `GripperBackend`.

    AGX's `get_gripper_status()` exposes width (`value`) and force only;
    velocity is not reported, so `get_gripper_velocity_m_per_s` always
    returns None and the runtime simply skips publishing the velocity
    state for that tick.
    """

    def __init__(self, gripper: Any) -> None:
        self._gripper = gripper

    def _status(self) -> Any | None:
        return self._gripper.get_gripper_status()

    def get_gripper_position_m(self) -> float | None:
        gs = self._status()
        if gs is None or gs.msg.mode != "width":
            return None
        return float(gs.msg.value)

    def get_gripper_velocity_m_per_s(self) -> float | None:
        # The AGX gripper firmware does not expose a velocity channel; the
        # state topic is left empty rather than fabricating a value via
        # numerical differencing (the controller would publish noisy data
        # at our control rate).
        return None

    def get_gripper_effort_n(self) -> float | None:
        gs = self._status()
        if gs is None:
            return None
        return float(gs.msg.force)

    def move_gripper_m(self, value: float, force: float) -> None:
        self._gripper.move_gripper_m(value=value, force=force)


__all__ = [
    "create_robot",
    "enable_robot",
    "init_gripper",
    "AgxArmBackend",
    "AgxGripperBackend",
]

"""Build the `DeviceQueryResponse` JSON for `rollio-device-nero query`.

The shape mirrors the serde JSON of `rollio_types::config::DeviceQueryResponse`
so the controller's setup wizard / config writer can consume the output
without an adapter. Reference:
../../../../../rollio-types/src/config.rs (`DeviceQueryResponse`,
`DeviceQueryDevice`, `DeviceQueryChannel`, `StateValueLimitsEntry`).
"""

from __future__ import annotations

import math
from typing import Any

from . import ARM_DOF, DEVICE_LABEL, DRIVER_NAME, GRIPPER_DOF
from .config import DEFAULT_CONTROL_FREQUENCY_HZ, TAU_MAX

# AGX Nero arm joint position limits (radians) extracted from the bundled
# `nero_description.urdf`. velocity / effort caps come from the URDF and the
# Nero firmware <= v110 t_ff range table.
ARM_JOINT_POSITION_MIN: tuple[float, ...] = (
    -2.70526,  # joint1
    -1.74,     # joint2
    -2.75,     # joint3
    -1.01,     # joint4
    -2.75,     # joint5
    -0.73,     # joint6
    -math.pi / 2,  # joint7 = -1.5707963
)
ARM_JOINT_POSITION_MAX: tuple[float, ...] = (
    2.70526,
    1.74,
    2.75,
    2.14,
    2.75,
    0.95,
    math.pi / 2,
)
ARM_JOINT_VELOCITY_BOUND: float = 5.0   # rad/s, URDF
ARM_JOINT_EFFORT_BOUND: tuple[float, ...] = TAU_MAX

# End-effector pose envelope (a generous bounding box centred on the base).
# Consumers (visualizer) just use this for axis scaling.
ARM_EE_POSE_MIN: tuple[float, ...] = (-0.8, -0.8, -0.2, -1.0, -1.0, -1.0, -1.0)
ARM_EE_POSE_MAX: tuple[float, ...] = (0.8, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0)

# AGX gripper width (m) and a conservative velocity / effort envelope.
GRIPPER_POSITION_MIN: float = 0.0
GRIPPER_POSITION_MAX: float = 0.07
GRIPPER_VELOCITY_BOUND: float = 0.5  # m/s
GRIPPER_EFFORT_BOUND: float = 10.0   # N

ARM_MODES: list[str] = ["free-drive", "command-following", "identifying", "disabled"]
GRIPPER_MODES: list[str] = ["free-drive", "command-following", "identifying", "disabled"]

ARM_SUPPORTED_STATES: list[str] = [
    "joint_position",
    "joint_velocity",
    "joint_effort",
    "end_effector_pose",
]
ARM_SUPPORTED_COMMANDS: list[str] = ["joint_position", "joint_mit", "end_pose"]

GRIPPER_SUPPORTED_STATES: list[str] = [
    "parallel_position",
    "parallel_velocity",
    "parallel_effort",
]
GRIPPER_SUPPORTED_COMMANDS: list[str] = ["parallel_position", "parallel_mit"]

ARM_DEFAULT_NAME: str = "agx_nero_arm"
GRIPPER_DEFAULT_NAME: str = "agx_nero_gripper"


def build_device_query_response(device_id: str) -> dict[str, Any]:
    """Return a dict matching the serde JSON of `DeviceQueryResponse`.

    `device_id` is the CAN interface (e.g. `can0`); we propagate it into
    `optional_info.interface` so the controller can persist it on the
    `BinaryDeviceConfig.extra` table when writing config.toml.
    """
    return {
        "driver": DRIVER_NAME,
        "devices": [_build_device(device_id)],
    }


def _build_device(device_id: str) -> dict[str, Any]:
    return {
        "id": device_id,
        "device_class": DRIVER_NAME,
        "device_label": DEVICE_LABEL,
        # Default user-facing name for the device row when the wizard
        # collapses arm + gripper into a single entry. The controller
        # falls back to a snake-case driver name when this is absent.
        "default_device_name": "agx_nero",
        "optional_info": {
            "interface": device_id,
            "transport": "can",
        },
        "channels": [_build_arm_channel(), _build_gripper_channel()],
    }


def _build_arm_channel() -> dict[str, Any]:
    return {
        "channel_type": "arm",
        "kind": "robot",
        "available": True,
        "channel_label": DEVICE_LABEL,
        "default_name": ARM_DEFAULT_NAME,
        "modes": list(ARM_MODES),
        "profiles": [],
        "supported_states": list(ARM_SUPPORTED_STATES),
        "supported_commands": list(ARM_SUPPORTED_COMMANDS),
        "supports_fk": True,
        "supports_ik": True,
        "dof": ARM_DOF,
        "default_control_frequency_hz": DEFAULT_CONTROL_FREQUENCY_HZ,
        "direct_joint_compatibility": {
            "can_lead": [{"driver": DRIVER_NAME, "channel_type": "arm"}],
            "can_follow": [{"driver": DRIVER_NAME, "channel_type": "arm"}],
        },
        "defaults": _empty_command_defaults(),
        "value_limits": _arm_value_limits(),
        "optional_info": {},
    }


def _build_gripper_channel() -> dict[str, Any]:
    return {
        "channel_type": "gripper",
        "kind": "robot",
        "available": True,
        "channel_label": f"{DEVICE_LABEL} gripper",
        "default_name": GRIPPER_DEFAULT_NAME,
        "modes": list(GRIPPER_MODES),
        "profiles": [],
        "supported_states": list(GRIPPER_SUPPORTED_STATES),
        "supported_commands": list(GRIPPER_SUPPORTED_COMMANDS),
        "supports_fk": False,
        "supports_ik": False,
        "dof": GRIPPER_DOF,
        "default_control_frequency_hz": DEFAULT_CONTROL_FREQUENCY_HZ,
        # Default Nero gripper PD: kp = closing force in N, kd left at 0.5
        # to mirror the airbot G2 convention so the two grippers are
        # interchangeable in pairing rules.
        "direct_joint_compatibility": {
            "can_lead": [{"driver": DRIVER_NAME, "channel_type": "gripper"}],
            "can_follow": [{"driver": DRIVER_NAME, "channel_type": "gripper"}],
        },
        "defaults": {
            "joint_mit_kp": [],
            "joint_mit_kd": [],
            "parallel_mit_kp": [10.0],
            "parallel_mit_kd": [0.5],
        },
        "value_limits": _gripper_value_limits(),
        "optional_info": {},
    }


def _empty_command_defaults() -> dict[str, list[float]]:
    return {
        "joint_mit_kp": [],
        "joint_mit_kd": [],
        "parallel_mit_kp": [],
        "parallel_mit_kd": [],
    }


def _arm_value_limits() -> list[dict[str, Any]]:
    return [
        {
            "state_kind": "joint_position",
            "min": list(ARM_JOINT_POSITION_MIN),
            "max": list(ARM_JOINT_POSITION_MAX),
        },
        {
            "state_kind": "joint_velocity",
            "min": [-ARM_JOINT_VELOCITY_BOUND] * ARM_DOF,
            "max": [ARM_JOINT_VELOCITY_BOUND] * ARM_DOF,
        },
        {
            "state_kind": "joint_effort",
            "min": [-bound for bound in ARM_JOINT_EFFORT_BOUND],
            "max": list(ARM_JOINT_EFFORT_BOUND),
        },
        {
            "state_kind": "end_effector_pose",
            "min": list(ARM_EE_POSE_MIN),
            "max": list(ARM_EE_POSE_MAX),
        },
    ]


def _gripper_value_limits() -> list[dict[str, Any]]:
    return [
        {
            "state_kind": "parallel_position",
            "min": [GRIPPER_POSITION_MIN],
            "max": [GRIPPER_POSITION_MAX],
        },
        {
            "state_kind": "parallel_velocity",
            "min": [-GRIPPER_VELOCITY_BOUND],
            "max": [GRIPPER_VELOCITY_BOUND],
        },
        {
            "state_kind": "parallel_effort",
            "min": [-GRIPPER_EFFORT_BOUND],
            "max": [GRIPPER_EFFORT_BOUND],
        },
    ]


# Re-exported so callers can do `from .query import ARM_JOINT_POSITION_MIN`
# without re-importing from `query` -- the runtime needs these limits to
# clip incoming command targets.
ARM_DOF_VALUE: int = ARM_DOF


__all__ = [
    "ARM_DOF_VALUE",
    "ARM_JOINT_POSITION_MIN",
    "ARM_JOINT_POSITION_MAX",
    "ARM_JOINT_VELOCITY_BOUND",
    "ARM_JOINT_EFFORT_BOUND",
    "ARM_EE_POSE_MIN",
    "ARM_EE_POSE_MAX",
    "GRIPPER_POSITION_MIN",
    "GRIPPER_POSITION_MAX",
    "GRIPPER_VELOCITY_BOUND",
    "GRIPPER_EFFORT_BOUND",
    "ARM_MODES",
    "GRIPPER_MODES",
    "ARM_SUPPORTED_STATES",
    "ARM_SUPPORTED_COMMANDS",
    "GRIPPER_SUPPORTED_STATES",
    "GRIPPER_SUPPORTED_COMMANDS",
    "ARM_DEFAULT_NAME",
    "GRIPPER_DEFAULT_NAME",
    "build_device_query_response",
]

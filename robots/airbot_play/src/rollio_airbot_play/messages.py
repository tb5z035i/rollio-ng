from __future__ import annotations

import ctypes
from collections.abc import Iterable
from dataclasses import dataclass
from enum import IntEnum
from typing import ClassVar

MAX_JOINTS = 16


class CommandMode(IntEnum):
    JOINT = 0
    CARTESIAN = 1


class ControlEventTag(IntEnum):
    RECORDING_START = 0
    RECORDING_STOP = 1
    EPISODE_KEEP = 2
    EPISODE_DISCARD = 3
    SHUTDOWN = 4
    MODE_SWITCH = 5


class RobotState(ctypes.Structure):
    _fields_: ClassVar[list[tuple[str, type[object]]]] = [
        ("timestamp_ns", ctypes.c_uint64),
        ("num_joints", ctypes.c_uint32),
        ("positions", ctypes.c_double * MAX_JOINTS),
        ("velocities", ctypes.c_double * MAX_JOINTS),
        ("efforts", ctypes.c_double * MAX_JOINTS),
        ("ee_pose", ctypes.c_double * 7),
        ("has_ee_pose", ctypes.c_bool),
    ]

    @staticmethod
    def type_name() -> str:
        return "RobotState"


class RobotCommand(ctypes.Structure):
    _fields_: ClassVar[list[tuple[str, type[object]]]] = [
        ("timestamp_ns", ctypes.c_uint64),
        ("mode", ctypes.c_uint32),
        ("num_joints", ctypes.c_uint32),
        ("joint_targets", ctypes.c_double * MAX_JOINTS),
        ("cartesian_target", ctypes.c_double * 7),
    ]

    @staticmethod
    def type_name() -> str:
        return "RobotCommand"


class _ControlEventPayload(ctypes.Union):
    _fields_: ClassVar[list[tuple[str, type[object]]]] = [
        ("episode_index", ctypes.c_uint32),
        ("target_mode", ctypes.c_uint32),
    ]


class ControlEvent(ctypes.Structure):
    _fields_: ClassVar[list[tuple[str, type[object]]]] = [
        ("tag", ctypes.c_uint32),
        ("payload", _ControlEventPayload),
    ]

    @staticmethod
    def type_name() -> str:
        return "ControlEvent"


@dataclass(slots=True)
class JointStateSnapshot:
    positions: list[float]
    velocities: list[float]
    efforts: list[float]

    def padded(self, dof: int) -> JointStateSnapshot:
        return JointStateSnapshot(
            positions=_pad(self.positions, dof),
            velocities=_pad(self.velocities, dof),
            efforts=_pad(self.efforts, dof),
        )


def build_robot_state_message(
    *,
    timestamp_ns: int,
    dof: int,
    snapshot: JointStateSnapshot,
) -> RobotState:
    message = RobotState()
    message.timestamp_ns = timestamp_ns
    message.num_joints = dof

    padded_snapshot = snapshot.padded(dof)
    for idx, value in enumerate(padded_snapshot.positions):
        message.positions[idx] = value
    for idx, value in enumerate(padded_snapshot.velocities):
        message.velocities[idx] = value
    for idx, value in enumerate(padded_snapshot.efforts):
        message.efforts[idx] = value

    return message


def command_targets(command: RobotCommand, dof: int) -> list[float]:
    active_joints = min(dof, int(command.num_joints), MAX_JOINTS)
    if CommandMode(command.mode) is CommandMode.JOINT:
        values = [float(command.joint_targets[idx]) for idx in range(active_joints)]
    else:
        values = [float(command.cartesian_target[idx]) for idx in range(min(active_joints, 7))]
    return _pad(values, dof)


def _pad(values: Iterable[float], dof: int) -> list[float]:
    padded = list(values)[:dof]
    if len(padded) < dof:
        padded.extend([0.0] * (dof - len(padded)))
    if len(padded) < MAX_JOINTS:
        padded.extend([0.0] * (MAX_JOINTS - len(padded)))
    return padded[:MAX_JOINTS]

"""ctypes mirrors of `rollio-types::messages` Rust structs.

The iceoryx2 Python bindings recognise a `type_name()` classmethod on a
`ctypes.Structure` payload and use `ctypes.sizeof()` / `ctypes.alignment()`
to negotiate type compatibility with the Rust side. Mirroring the Rust
`#[repr(C)]` layouts here therefore makes the Python device wire-compatible
with the Rust controller / visualizer / pairing modules without any
upstream changes.

Reference: ../../../../../../rollio-types/src/messages.rs
"""

from __future__ import annotations

import ctypes
from typing import ClassVar

# ---------------------------------------------------------------------------
# Shared shape constants -- must match `rollio-types::messages`
# ---------------------------------------------------------------------------

MAX_DOF: int = 15
MAX_PARALLEL: int = 2
MAX_POSE: int = 7

# Rust `#[repr(C)] pub enum DeviceChannelMode` discriminant values. We re-export
# them as module-level constants so the runtime can dispatch on `int(value)`
# without importing ctypes.
DEVICE_CHANNEL_MODE_DISABLED: int = 0
DEVICE_CHANNEL_MODE_ENABLED: int = 1
DEVICE_CHANNEL_MODE_FREE_DRIVE: int = 2
DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING: int = 3
DEVICE_CHANNEL_MODE_IDENTIFYING: int = 4

# `ControlEvent::Shutdown` discriminant. The other variants are not consumed
# by device drivers; per-channel mode switches arrive on the channel-specific
# `control/mode` topic (see `ipc.services`), not on the global control bus.
CONTROL_EVENT_RECORDING_START: int = 0
CONTROL_EVENT_RECORDING_STOP: int = 1
CONTROL_EVENT_EPISODE_KEEP: int = 2
CONTROL_EVENT_EPISODE_DISCARD: int = 3
CONTROL_EVENT_SHUTDOWN: int = 4
CONTROL_EVENT_MODE_SWITCH: int = 5


# ---------------------------------------------------------------------------
# Vectors and commands
# ---------------------------------------------------------------------------


class JointVector15(ctypes.Structure):
    """Mirror of Rust `JointVector15` (`repr(C)`).

    Layout: `u64 timestamp_ms` (0..8), `u32 len` (8..12), 4-byte padding,
    `f64[15] values` (16..136). Total 136 bytes, alignment 8.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("timestamp_ms", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("values", ctypes.c_double * MAX_DOF),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "JointVector15"

    @classmethod
    def from_values(cls, timestamp_ms: int, values: list[float]) -> JointVector15:
        msg = cls()
        msg.timestamp_ms = int(timestamp_ms) & 0xFFFFFFFFFFFFFFFF
        n = min(len(values), MAX_DOF)
        msg.len = n
        for i in range(n):
            msg.values[i] = float(values[i])
        return msg


class JointMitCommand15(ctypes.Structure):
    """Mirror of Rust `JointMitCommand15` (`repr(C)`).

    Layout: `u64 timestamp_ms`, `u32 len`, 4-byte padding, then five
    `f64[15]` arrays (position, velocity, effort, kp, kd). Total 616 bytes.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("timestamp_ms", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("position", ctypes.c_double * MAX_DOF),
        ("velocity", ctypes.c_double * MAX_DOF),
        ("effort", ctypes.c_double * MAX_DOF),
        ("kp", ctypes.c_double * MAX_DOF),
        ("kd", ctypes.c_double * MAX_DOF),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "JointMitCommand15"


class Pose7(ctypes.Structure):
    """Mirror of Rust `Pose7` (`repr(C)`).

    Layout: `u64 timestamp_ms` (0..8), `f64[7] values` (8..64). Total 64 bytes.
    Values are `[x, y, z, qx, qy, qz, qw]` with the quaternion in xyzw order.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("timestamp_ms", ctypes.c_uint64),
        ("values", ctypes.c_double * MAX_POSE),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "Pose7"

    @classmethod
    def from_values(cls, timestamp_ms: int, values: list[float]) -> Pose7:
        msg = cls()
        msg.timestamp_ms = int(timestamp_ms) & 0xFFFFFFFFFFFFFFFF
        for i in range(min(len(values), MAX_POSE)):
            msg.values[i] = float(values[i])
        return msg


class ParallelVector2(ctypes.Structure):
    """Mirror of Rust `ParallelVector2` (`repr(C)`).

    Layout: `u64 timestamp_ms`, `u32 len`, 4-byte padding, `f64[2] values`.
    Total 32 bytes.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("timestamp_ms", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("values", ctypes.c_double * MAX_PARALLEL),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "ParallelVector2"

    @classmethod
    def from_values(cls, timestamp_ms: int, values: list[float]) -> ParallelVector2:
        msg = cls()
        msg.timestamp_ms = int(timestamp_ms) & 0xFFFFFFFFFFFFFFFF
        n = min(len(values), MAX_PARALLEL)
        msg.len = n
        for i in range(n):
            msg.values[i] = float(values[i])
        return msg


class ParallelMitCommand2(ctypes.Structure):
    """Mirror of Rust `ParallelMitCommand2` (`repr(C)`).

    Layout: `u64 timestamp_ms`, `u32 len`, 4-byte padding, then five
    `f64[2]` arrays (position, velocity, effort, kp, kd). Total 96 bytes.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("timestamp_ms", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("position", ctypes.c_double * MAX_PARALLEL),
        ("velocity", ctypes.c_double * MAX_PARALLEL),
        ("effort", ctypes.c_double * MAX_PARALLEL),
        ("kp", ctypes.c_double * MAX_PARALLEL),
        ("kd", ctypes.c_double * MAX_PARALLEL),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "ParallelMitCommand2"


# ---------------------------------------------------------------------------
# Enums (modelled as a single-field Structure so iceoryx2's get_type_name +
# ctypes.sizeof / alignment plumbing accepts them).
# ---------------------------------------------------------------------------


class DeviceChannelMode(ctypes.Structure):
    """Mirror of Rust `#[repr(C)] enum DeviceChannelMode` (single i32 discriminant).

    The Rust enum has no payload variants, so its `repr(C)` representation is
    a plain `c_int` on every platform we run on (x86_64 / aarch64 Linux,
    macOS). Size 4, alignment 4.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("value", ctypes.c_int32),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "DeviceChannelMode"

    @classmethod
    def of(cls, discriminant: int) -> DeviceChannelMode:
        msg = cls()
        msg.value = int(discriminant)
        return msg


class _ControlEventPayload(ctypes.Union):
    """Anonymous union of Rust `ControlEvent` payloads (all `u32`)."""

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("episode_index", ctypes.c_uint32),
        ("target_mode", ctypes.c_uint32),
    ]


class ControlEvent(ctypes.Structure):
    """Mirror of Rust `#[repr(C)] enum ControlEvent` (tag + payload union).

    For `repr(C)` enums with at least one variant carrying data, Rust lays
    them out as `struct { tag: c_int; payload: union<...> }`. Every variant
    we care about either carries no data (`Shutdown`) or a single `u32`, so
    the union collapses to a 4-byte u32. Total size 8, alignment 4.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("tag", ctypes.c_int32),
        ("payload", _ControlEventPayload),
    ]

    @classmethod
    def type_name(cls) -> str:
        return "ControlEvent"


__all__ = [
    "CONTROL_EVENT_EPISODE_DISCARD",
    "CONTROL_EVENT_EPISODE_KEEP",
    "CONTROL_EVENT_MODE_SWITCH",
    "CONTROL_EVENT_RECORDING_START",
    "CONTROL_EVENT_RECORDING_STOP",
    "CONTROL_EVENT_SHUTDOWN",
    "DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING",
    "DEVICE_CHANNEL_MODE_DISABLED",
    "DEVICE_CHANNEL_MODE_ENABLED",
    "DEVICE_CHANNEL_MODE_FREE_DRIVE",
    "DEVICE_CHANNEL_MODE_IDENTIFYING",
    "MAX_DOF",
    "MAX_PARALLEL",
    "MAX_POSE",
    "ControlEvent",
    "DeviceChannelMode",
    "JointMitCommand15",
    "JointVector15",
    "ParallelMitCommand2",
    "ParallelVector2",
    "Pose7",
]

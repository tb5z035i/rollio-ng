"""Wire-format compatibility checks for `rollio_device_nero.ipc.types`.

The `_RUST_*` constants below are the canonical sizes / alignments / field
offsets reported by `std::mem::size_of`, `align_of` and `offset_of` on
`rollio-types::messages` -- captured once via a tiny one-off Rust binary
and frozen here so this test can run without cargo.

If a mirrored ctypes layout drifts from the Rust struct (e.g. someone adds
a field on either side), this test will catch it before the mismatch
silently corrupts every iceoryx2 sample at runtime.
"""

from __future__ import annotations

import ctypes

import pytest
from rollio_device_nero.ipc import types as t

# Captured 2026-04-19 from rollio-types HEAD via:
#     std::mem::{size_of, align_of}::<T>(), std::mem::offset_of!(T, field)
_RUST_LAYOUTS: dict[type, tuple[int, int]] = {
    t.JointVector15: (136, 8),
    t.JointMitCommand15: (616, 8),
    t.Pose7: (64, 8),
    t.ParallelVector2: (32, 8),
    t.ParallelMitCommand2: (96, 8),
    t.DeviceChannelMode: (4, 4),
    t.ControlEvent: (8, 4),
}


@pytest.mark.parametrize("cls,expected", list(_RUST_LAYOUTS.items()))
def test_size_and_alignment_match_rust(cls: type, expected: tuple[int, int]) -> None:
    assert (ctypes.sizeof(cls), ctypes.alignment(cls)) == expected


def test_joint_vector_15_field_offsets() -> None:
    assert t.JointVector15.timestamp_ms.offset == 0
    assert t.JointVector15.len.offset == 8
    # 4 bytes of natural alignment padding after `len`.
    assert t.JointVector15.values.offset == 16


def test_joint_mit_command_15_field_offsets() -> None:
    assert t.JointMitCommand15.timestamp_ms.offset == 0
    assert t.JointMitCommand15.len.offset == 8
    assert t.JointMitCommand15.position.offset == 16
    assert t.JointMitCommand15.velocity.offset == 16 + 15 * 8
    assert t.JointMitCommand15.effort.offset == 16 + 2 * 15 * 8
    assert t.JointMitCommand15.kp.offset == 16 + 3 * 15 * 8
    assert t.JointMitCommand15.kd.offset == 16 + 4 * 15 * 8


def test_pose7_field_offsets() -> None:
    assert t.Pose7.timestamp_ms.offset == 0
    assert t.Pose7.values.offset == 8


def test_parallel_vector_2_field_offsets() -> None:
    assert t.ParallelVector2.timestamp_ms.offset == 0
    assert t.ParallelVector2.len.offset == 8
    assert t.ParallelVector2.values.offset == 16


def test_control_event_field_offsets() -> None:
    assert t.ControlEvent.tag.offset == 0
    assert t.ControlEvent.payload.offset == 4


@pytest.mark.parametrize(
    "cls,expected_name",
    [
        (t.JointVector15, "JointVector15"),
        (t.JointMitCommand15, "JointMitCommand15"),
        (t.Pose7, "Pose7"),
        (t.ParallelVector2, "ParallelVector2"),
        (t.ParallelMitCommand2, "ParallelMitCommand2"),
        (t.DeviceChannelMode, "DeviceChannelMode"),
        (t.ControlEvent, "ControlEvent"),
    ],
)
def test_type_name_matches_rust(cls: type, expected_name: str) -> None:
    """`get_type_name` from iceoryx2's helpers calls `cls.type_name()`."""
    assert cls.type_name() == expected_name  # type: ignore[attr-defined]


def test_joint_vector_15_from_values_truncates_and_pads() -> None:
    msg = t.JointVector15.from_values(timestamp_ms=42, values=[1.0, 2.0, 3.0])
    assert msg.timestamp_ms == 42
    assert msg.len == 3
    assert msg.values[0] == 1.0
    assert msg.values[2] == 3.0
    assert msg.values[3] == 0.0  # untouched slot stays zero


def test_pose7_from_values_pads_short_inputs() -> None:
    msg = t.Pose7.from_values(timestamp_ms=99, values=[0.1, 0.2, 0.3])
    assert msg.timestamp_ms == 99
    assert msg.values[0] == 0.1
    assert msg.values[6] == 0.0


def test_device_channel_mode_constants_match_rust_discriminants() -> None:
    """Rust `repr(C) enum DeviceChannelMode` assigns 0..4 in declaration order."""
    assert t.DEVICE_CHANNEL_MODE_DISABLED == 0
    assert t.DEVICE_CHANNEL_MODE_ENABLED == 1
    assert t.DEVICE_CHANNEL_MODE_FREE_DRIVE == 2
    assert t.DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING == 3
    assert t.DEVICE_CHANNEL_MODE_IDENTIFYING == 4


def test_control_event_shutdown_discriminant_is_4() -> None:
    """Rust `repr(C) enum ControlEvent` declares Shutdown as the 5th variant."""
    assert t.CONTROL_EVENT_SHUTDOWN == 4

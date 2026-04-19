"""Tests for `rollio_device_nero.config`."""

from __future__ import annotations

import pytest

from rollio_device_nero.config import (
    ARM_CHANNEL_TYPE,
    CONTROL_FREQUENCY_HZ,
    ConfigError,
    GRIPPER_CHANNEL_TYPE,
    parse_runtime_config,
)


def _minimal_config(**overrides):
    base = {
        "name": "agx_nero",
        "driver": "agx-nero",
        "id": "can0",
        "bus_root": "agx_nero",
        "interface": "can0",
        "channels": [
            {
                "channel_type": "arm",
                "kind": "robot",
                "mode": "free-drive",
                "dof": 7,
                "publish_states": ["joint_position", "joint_velocity"],
            },
            {
                "channel_type": "gripper",
                "kind": "robot",
                "mode": "command-following",
                "publish_states": ["parallel_position"],
                "command_defaults": {"parallel_mit_kp": [4.0]},
            },
        ],
    }
    base.update(overrides)
    return base


def test_parse_runtime_config_extracts_arm_and_gripper() -> None:
    cfg = parse_runtime_config(_minimal_config())
    assert cfg.bus_root == "agx_nero"
    assert cfg.device_id == "can0"
    assert cfg.interface == "can0"
    assert cfg.arm is not None
    assert cfg.arm.channel_type == ARM_CHANNEL_TYPE
    assert cfg.arm.mode == "free-drive"
    assert cfg.arm.dof == 7
    assert cfg.arm.publish_states == ["joint_position", "joint_velocity"]

    assert cfg.gripper is not None
    assert cfg.gripper.channel_type == GRIPPER_CHANNEL_TYPE
    assert cfg.gripper.mode == "command-following"
    # Force-default seeded from the controller's `parallel_mit_kp[0]`.
    assert cfg.gripper.default_force_n == 4.0


def test_control_frequency_hz_is_a_compile_time_constant() -> None:
    """The runtime locks both channels to `CONTROL_FREQUENCY_HZ` regardless
    of any per-channel `control_frequency_hz` the controller emitted in
    its TOML. The constant must match the AIRBOT Play `CONTROL_HZ` (250)
    so the per-tick safety clamp gives the same effective slew cap on
    both robots; if this assertion ever fails, the rationale chain in
    `runtime/arm.py::MAX_COMMAND_JOINT_DELTA_RAD` needs revisiting."""
    assert CONTROL_FREQUENCY_HZ == 250.0


def test_control_frequency_hz_in_toml_is_silently_ignored() -> None:
    """The Nero runtime no longer honours per-channel `control_frequency_hz`
    overrides from the TOML. The parser must accept the field for
    backward compatibility with controller-emitted configs but the
    parsed dataclass must not surface it (the field was removed
    entirely so misconfigurations cannot accidentally slow the loop)."""
    cfg = parse_runtime_config(
        _minimal_config(
            channels=[
                {
                    "channel_type": "arm",
                    "kind": "robot",
                    "mode": "free-drive",
                    "dof": 7,
                    "control_frequency_hz": 50.0,  # ignored
                },
                {
                    "channel_type": "gripper",
                    "kind": "robot",
                    "mode": "command-following",
                    "control_frequency_hz": 17.0,  # ignored
                },
            ]
        )
    )
    assert cfg.arm is not None
    assert not hasattr(cfg.arm, "control_frequency_hz"), (
        "ArmChannelConfig.control_frequency_hz was removed; tests must "
        "not depend on the runtime honouring a TOML override"
    )
    assert cfg.gripper is not None
    assert not hasattr(cfg.gripper, "control_frequency_hz")


def test_gripper_default_force_falls_back_to_firmware_max_when_unset() -> None:
    """Operators who don't supply `command_defaults.parallel_mit_kp[0]`
    in the controller TOML must still get the snappy 3.0 N firmware-max
    default (anything below saturates the AGX gripper's force-driven
    speed; see GripperChannelConfig.default_force_n)."""
    cfg = parse_runtime_config(
        _minimal_config(
            channels=[
                {
                    "channel_type": "arm",
                    "kind": "robot",
                    "mode": "free-drive",
                    "dof": 7,
                },
                {
                    "channel_type": "gripper",
                    "kind": "robot",
                    "mode": "command-following",
                },
            ]
        )
    )
    assert cfg.gripper is not None
    assert cfg.gripper.default_force_n == 3.0


def test_disabled_channels_are_dropped() -> None:
    cfg = parse_runtime_config(
        _minimal_config(
            channels=[
                {"channel_type": "arm", "kind": "robot", "mode": "free-drive", "enabled": True},
                {"channel_type": "gripper", "kind": "robot", "enabled": False},
            ]
        )
    )
    assert cfg.arm is not None
    assert cfg.gripper is None


def test_rejects_wrong_driver() -> None:
    with pytest.raises(ConfigError, match=r"agx-nero"):
        parse_runtime_config(_minimal_config(driver="airbot-play"))


def test_rejects_unknown_mode() -> None:
    with pytest.raises(ConfigError, match=r"unsupported mode"):
        parse_runtime_config(
            _minimal_config(
                channels=[
                    {"channel_type": "arm", "kind": "robot", "mode": "yoga"},
                ]
            )
        )


def test_rejects_dof_other_than_7() -> None:
    with pytest.raises(ConfigError, match=r"dof must be 7"):
        parse_runtime_config(
            _minimal_config(
                channels=[
                    {"channel_type": "arm", "kind": "robot", "mode": "free-drive", "dof": 6},
                ]
            )
        )


def test_rejects_non_robot_kind() -> None:
    with pytest.raises(ConfigError, match=r"kind must be"):
        parse_runtime_config(
            _minimal_config(
                channels=[
                    {"channel_type": "arm", "kind": "camera", "mode": "free-drive"},
                ]
            )
        )


def test_no_enabled_channels_is_an_error() -> None:
    with pytest.raises(ConfigError, match=r"no enabled"):
        parse_runtime_config(
            _minimal_config(
                channels=[
                    {"channel_type": "arm", "kind": "robot", "enabled": False},
                ]
            )
        )


def test_unknown_channel_type_is_silently_skipped() -> None:
    cfg = parse_runtime_config(
        _minimal_config(
            channels=[
                {"channel_type": "arm", "kind": "robot", "mode": "free-drive"},
                {"channel_type": "future-camera-attachment", "kind": "robot"},
            ]
        )
    )
    assert cfg.arm is not None
    assert cfg.gripper is None

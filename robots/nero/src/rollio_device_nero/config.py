"""TOML config parser for `rollio-device-nero run`.

Mirrors the shape of `rollio-types::config::BinaryDeviceConfig` (the schema
the controller writes when it spawns the device executable) just closely
enough to extract the fields this driver actually consumes. We do not
re-validate the controller's TOML invariants -- the controller has
already done that with the canonical Rust validator.

Reference: ../../../../../rollio-types/src/config.rs (`BinaryDeviceConfig` /
`DeviceChannelConfigV2`).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python 3.10 fallback
    import tomli as tomllib  # type: ignore[import-not-found,no-redef]

from . import DRIVER_NAME

ARM_CHANNEL_TYPE: str = "arm"
GRIPPER_CHANNEL_TYPE: str = "gripper"

# Hard-coded control loop rate for the Nero arm and gripper runtimes.
# Mirrors AIRBOT Play's `CONTROL_HZ` so the per-tick safety clamp
# (`MAX_COMMAND_JOINT_DELTA_RAD = pi/36 rad`, ~5 deg) yields the same
# effective slew cap (~21.8 rad/s) on both robots. The runtime warns if
# it cannot keep up with this rate (see `runtime.rate_monitor`).
#
# The TOML config that the controller writes still carries a
# `control_frequency_hz` field per channel, but the Nero device driver
# now ignores it -- the rate is a deployment constant, not a per-channel
# tunable, exactly as on AIRBOT.
CONTROL_FREQUENCY_HZ: float = 250.0

# Deprecated alias retained so that `query.py` (which still serves it as
# `default_control_frequency_hz` to the controller) and any other older
# callers keep working without churn. Equal to `CONTROL_FREQUENCY_HZ`.
DEFAULT_CONTROL_FREQUENCY_HZ: float = CONTROL_FREQUENCY_HZ

# Warn when the achieved control rate over a recent window drops below
# this fraction of the target. 0.95 catches sustained slowdowns without
# tripping on isolated jitter.
MIN_ACHIEVED_FREQUENCY_RATIO: float = 0.95

# Per-mode arm gain defaults.
#
#   * CommandFollowing / Disabled-hold: (kp=10, kd=0.5) per the user spec.
#   * FreeDrive: (kp=0, kd=0) -- truly floating arm. Only the gravity
#     feed-forward is commanded, so the operator can move it by hand
#     without fighting MIT damping.
#   * Identifying: (kp=0, kd=0) -- same control shape as FreeDrive; only
#     the reported mode differs so the rollio setup wizard can highlight it.
#
# The two constants are kept separate so future operator-tunable damping
# (e.g. add a tiny kd to Identifying for a "shake-test" mode) only touches
# the corresponding branch.
DEFAULT_TRACKING_KP: float = 10.0
DEFAULT_TRACKING_KD: float = 1.0
DEFAULT_FREE_DRIVE_KD: float = 0.0
DEFAULT_IDENTIFYING_KD: float = 0.0

# Per-joint torque saturation for the AGX Nero (firmware <= v110, NeroFW.DEFAULT).
# Source: docs/nero/nero_api.md - move_mit() t_ff range table.
TAU_MAX: tuple[float, ...] = (24.0, 24.0, 18.0, 18.0, 8.0, 8.0, 8.0)


class ConfigError(RuntimeError):
    """Raised when the device config cannot be parsed or is incomplete."""


@dataclass(slots=True)
class ArmChannelConfig:
    channel_type: str = ARM_CHANNEL_TYPE
    enabled: bool = True
    mode: str = "free-drive"
    dof: int = 7
    publish_states: list[str] = field(default_factory=list)


@dataclass(slots=True)
class GripperChannelConfig:
    channel_type: str = GRIPPER_CHANNEL_TYPE
    enabled: bool = True
    mode: str = "free-drive"
    publish_states: list[str] = field(default_factory=list)
    # Default close/open force used when the controller does not provide
    # one via `command_defaults.parallel_mit_kp[0]`. The AGX gripper
    # firmware exposes the full [0.0, 3.0] N envelope and the actuator's
    # closing speed scales with this force (it has no separate velocity
    # knob), so we default at the spec max for snappy teleop. Operators
    # who need delicate gripping should override via the controller's
    # `parallel_mit_kp[0]` channel default or per-command MIT `kp` slot.
    default_force_n: float = 3.0


@dataclass(slots=True)
class RuntimeConfig:
    bus_root: str
    device_id: str
    interface: str
    arm: ArmChannelConfig | None
    gripper: GripperChannelConfig | None


def load_runtime_config(
    *,
    config: Path | None,
    config_inline: str | None,
) -> RuntimeConfig:
    if (config is None) == (config_inline is None):
        raise ConfigError("run requires exactly one of --config or --config-inline")

    if config is not None:
        text = Path(config).read_text(encoding="utf-8")
    else:
        text = config_inline or ""

    data = tomllib.loads(text)
    return parse_runtime_config(data)


def parse_runtime_config(data: dict[str, Any]) -> RuntimeConfig:
    driver = _required_string(data, "driver")
    if driver != DRIVER_NAME:
        raise ConfigError(f'expected driver = "{DRIVER_NAME}", got "{driver}"')

    bus_root = _required_string(data, "bus_root")
    device_id = _required_string(data, "id")
    interface = _required_string(data, "interface")

    raw_channels = data.get("channels")
    if not isinstance(raw_channels, list) or not raw_channels:
        raise ConfigError("at least one [[channels]] entry is required")

    arm: ArmChannelConfig | None = None
    gripper: GripperChannelConfig | None = None

    for raw in raw_channels:
        if not isinstance(raw, dict):
            raise ConfigError("each [[channels]] entry must be a table")
        channel_type = _required_string(raw, "channel_type")
        kind = raw.get("kind", "robot")
        if kind != "robot":
            raise ConfigError(
                f'channel "{channel_type}": kind must be "robot", got "{kind}"'
            )
        enabled = bool(raw.get("enabled", True))
        if not enabled:
            continue

        if channel_type == ARM_CHANNEL_TYPE:
            arm = _parse_arm(raw)
        elif channel_type == GRIPPER_CHANNEL_TYPE:
            gripper = _parse_gripper(raw)
        else:
            # Unknown channel types are skipped so future driver extensions
            # don't break older Python builds.
            continue

    if arm is None and gripper is None:
        raise ConfigError(
            'no enabled "arm" or "gripper" channel; nothing to run'
        )

    return RuntimeConfig(
        bus_root=bus_root,
        device_id=device_id,
        interface=interface,
        arm=arm,
        gripper=gripper,
    )


def _parse_arm(raw: dict[str, Any]) -> ArmChannelConfig:
    mode = _normalize_mode(raw.get("mode", "free-drive"))
    dof = int(raw.get("dof", 7))
    if dof != 7:
        # Nero is fixed 7-DOF; reject misconfigured DOFs early so we don't
        # silently send commands the URDF / firmware can't fulfil.
        raise ConfigError(f'arm channel: dof must be 7 for AGX Nero (got {dof})')
    publish_states = _string_list(raw.get("publish_states", []))
    # Note: `control_frequency_hz` from the controller TOML is intentionally
    # ignored -- the runtime locks to `CONTROL_FREQUENCY_HZ` (250 Hz) so the
    # per-tick safety clamp gives the same effective slew cap as AIRBOT Play.
    return ArmChannelConfig(
        mode=mode,
        dof=dof,
        publish_states=publish_states,
    )


def _parse_gripper(raw: dict[str, Any]) -> GripperChannelConfig:
    mode = _normalize_mode(raw.get("mode", "free-drive"))
    publish_states = _string_list(raw.get("publish_states", []))

    # See `GripperChannelConfig.default_force_n` for the rationale -- the
    # firmware spec max is 3.0 N and the AGX gripper has no independent
    # speed knob, so the default is set there for snappy teleop.
    default_force_n = 3.0
    cmd_defaults = raw.get("command_defaults", {})
    if isinstance(cmd_defaults, dict):
        kp_list = cmd_defaults.get("parallel_mit_kp")
        if isinstance(kp_list, list) and kp_list:
            try:
                default_force_n = float(kp_list[0])
            except (TypeError, ValueError):
                pass

    # Note: `control_frequency_hz` from the controller TOML is intentionally
    # ignored -- the runtime locks to `CONTROL_FREQUENCY_HZ` (see arm parser).
    return GripperChannelConfig(
        mode=mode,
        publish_states=publish_states,
        default_force_n=default_force_n,
    )


_VALID_MODES: frozenset[str] = frozenset(
    {"free-drive", "command-following", "identifying", "disabled"}
)


def _normalize_mode(value: Any) -> str:
    if not isinstance(value, str):
        raise ConfigError(f"mode must be a string, got {type(value).__name__}")
    if value not in _VALID_MODES:
        raise ConfigError(
            f'unsupported mode "{value}" (must be one of '
            f'{sorted(_VALID_MODES)!r})'
        )
    return value


def _required_string(data: dict[str, Any], key: str) -> str:
    value = data.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ConfigError(f'"{key}" must be a non-empty string')
    return value


def _string_list(value: Any) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise ConfigError("publish_states must be a list of strings")
    out: list[str] = []
    for item in value:
        if not isinstance(item, str):
            raise ConfigError("publish_states entries must be strings")
        out.append(item)
    return out


__all__ = [
    "ARM_CHANNEL_TYPE",
    "GRIPPER_CHANNEL_TYPE",
    "CONTROL_FREQUENCY_HZ",
    "DEFAULT_CONTROL_FREQUENCY_HZ",
    "MIN_ACHIEVED_FREQUENCY_RATIO",
    "DEFAULT_TRACKING_KP",
    "DEFAULT_TRACKING_KD",
    "DEFAULT_FREE_DRIVE_KD",
    "DEFAULT_IDENTIFYING_KD",
    "TAU_MAX",
    "ConfigError",
    "ArmChannelConfig",
    "GripperChannelConfig",
    "RuntimeConfig",
    "load_runtime_config",
    "parse_runtime_config",
]

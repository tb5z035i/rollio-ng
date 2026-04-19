"""Discovery + validation entry points for the AGX Nero driver.

`pyAgxArm` is imported lazily so a host without it can still run unit tests
that exercise `probe`'s pure-Python iface enumeration (and so the help
output of `rollio-device-nero --help` works without the SDK).
"""

from __future__ import annotations

import os
import time
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path

DEFAULT_PROBE_TIMEOUT_MS: int = 1000


@dataclass(slots=True)
class ProbedDevice:
    """A single AGX Nero candidate detected by `probe`."""

    interface: str
    feedback_observed: bool

    @property
    def device_id(self) -> str:
        # The device id is the CAN interface name (`can0`, `can1`, ...) per
        # the user's spec: "If `can0` is not a valid Nero, validate fails."
        return self.interface


# ---------------------------------------------------------------------------
# CAN interface enumeration
# ---------------------------------------------------------------------------


_SYS_CLASS_NET = Path("/sys/class/net")


def list_can_interfaces() -> list[str]:
    """Enumerate kernel CAN interfaces in name order.

    Filters by `/sys/class/net/<name>/type == 280` (Linux ARPHRD_CAN). Falls
    back to a simple `name.startswith("can")` heuristic on platforms where
    the type file is unreadable so this still works inside containers with
    a mocked `/sys`.
    """
    if not _SYS_CLASS_NET.is_dir():
        return []

    interfaces: list[str] = []
    for entry in sorted(_SYS_CLASS_NET.iterdir()):
        name = entry.name
        type_file = entry / "type"
        try:
            arphrd = int(type_file.read_text().strip())
        except (OSError, ValueError):
            arphrd = None
        if arphrd == 280 or (arphrd is None and name.startswith("can")):
            interfaces.append(name)
    return interfaces


# ---------------------------------------------------------------------------
# Probe (non-invasive feedback check)
# ---------------------------------------------------------------------------


def probe_devices(timeout_ms: int = DEFAULT_PROBE_TIMEOUT_MS) -> list[ProbedDevice]:
    """Enumerate CAN interfaces and check each one for AGX Nero feedback.

    `connect()` opens the CAN socket and starts a read thread; it does NOT
    enable the motors, so it is safe to do on every iface. We then poll
    `get_joint_angles()` for up to `timeout_ms` and consider the iface a
    Nero candidate iff at least one valid joint-angles frame arrives.
    """
    interfaces = list_can_interfaces()
    if not interfaces:
        return []

    try:
        from pyAgxArm import (  # type: ignore[import-not-found]
            AgxArmFactory,
            ArmModel,
            NeroFW,
            create_agx_arm_config,
        )
    except Exception:
        # Without `pyAgxArm` we cannot verify, but we still report the ifaces
        # so `validate` (which can fail loudly) is the source of truth.
        return [ProbedDevice(interface=iface, feedback_observed=False) for iface in interfaces]

    out: list[ProbedDevice] = []
    deadline_per_iface = max(0.05, timeout_ms / 1000.0)
    for iface in interfaces:
        cfg = create_agx_arm_config(
            robot=ArmModel.NERO,
            firmeware_version=NeroFW.DEFAULT,
            channel=iface,
        )
        feedback = False
        robot = None
        try:
            robot = AgxArmFactory.create_arm(cfg)
            robot.connect()
            deadline = time.monotonic() + deadline_per_iface
            while time.monotonic() < deadline:
                ja = robot.get_joint_angles()
                if ja is not None:
                    feedback = True
                    break
                time.sleep(0.01)
        except Exception:
            feedback = False
        finally:
            if robot is not None:
                with suppress(Exception):
                    robot.disconnect()
        out.append(ProbedDevice(interface=iface, feedback_observed=feedback))
    return out


# ---------------------------------------------------------------------------
# Validate (invasive: connect + enable per test.py)
# ---------------------------------------------------------------------------


def validate_device(
    device_id: str,
    *,
    timeout_ms: int = DEFAULT_PROBE_TIMEOUT_MS,
) -> bool:
    """Run the canonical Nero readiness check from `external/reference/nero-demo/test.py`.

    Sequence:
        1. `connect()`
        2. loop `enable() / set_normal_mode()` every 10 ms until True or timeout
        3. `disconnect()`

    Returns True iff `enable()` reports all motors enabled within `timeout_ms`.
    """
    if not _looks_like_can_interface(device_id):
        raise RuntimeError(
            f"unknown AGX Nero device id: {device_id!r} (expected a CAN interface name like 'can0')"
        )

    try:
        from pyAgxArm import (  # type: ignore[import-not-found]
            AgxArmFactory,
            ArmModel,
            NeroFW,
            create_agx_arm_config,
        )
    except Exception as exc:
        raise RuntimeError(
            "pyAgxArm is required for validate; install the third_party/pyAgxArm submodule"
        ) from exc

    cfg = create_agx_arm_config(
        robot=ArmModel.NERO,
        firmeware_version=NeroFW.DEFAULT,
        channel=device_id,
    )
    robot = AgxArmFactory.create_arm(cfg)
    try:
        robot.connect()
        deadline = time.monotonic() + max(0.05, timeout_ms / 1000.0)
        while time.monotonic() < deadline:
            if robot.enable():
                return True
            robot.set_normal_mode()
            time.sleep(0.01)
        return False
    finally:
        with suppress(Exception):
            robot.disconnect()


def _looks_like_can_interface(name: str) -> bool:
    if not name or "/" in name or name in {".", ".."}:
        return False
    # `socket.if_nameindex()` would be authoritative but requires the iface
    # to actually be up; we just sanity-check the path is well-formed.
    return os.path.basename(name) == name


__all__ = [
    "DEFAULT_PROBE_TIMEOUT_MS",
    "ProbedDevice",
    "list_can_interfaces",
    "probe_devices",
    "validate_device",
]
